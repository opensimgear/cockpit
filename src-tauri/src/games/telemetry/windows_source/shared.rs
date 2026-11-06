use super::{Game, RawSample, Source, WindowsSource};
use crate::game_discovery::{normalized_path, ProcessIdentity};
use osg_streams::{Delivery, NativeLimits, NativeStream, NativeSubscription, RetainedBytes};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
    thread::{self, Thread},
    time::{Duration, Instant},
};

const SAMPLE_PERIOD: Duration = Duration::from_millis(2);
const RETRY_PERIOD: Duration = Duration::from_millis(50);
const LIMITS: NativeLimits = NativeLimits {
    max_subscribers: 16,
    max_snapshot_bytes: 16 * 1024,
    max_frame_bytes: 16 * 1024,
    max_pending_frames: 128,
    max_pending_bytes: 1024 * 1024,
};

#[derive(Clone)]
enum Frame {
    Waiting,
    Sample(Arc<RawSample>),
    Error(Arc<str>),
}

impl RetainedBytes for Frame {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Waiting => 0,
            Self::Sample(sample) => {
                sample.physics.len()
                    + sample.graphics.len()
                    + sample.statics.len()
                    + sample.source.id.len()
                    + 256
            }
            Self::Error(error) => error.len(),
        }
    }
}

type Stream = NativeStream<Frame, Frame>;
type Reader = NativeSubscription<Frame, Frame>;

pub(crate) struct Watch(osg_streams::NativeWatch<Frame, Frame>, Option<Weak<crate::xplane::process_events::Monitor>>);
impl Watch {
    pub(crate) fn is_closed(&self) -> bool {
        self.0.is_closed()
    }
    pub(crate) fn process_current(&self) -> Option<bool> {
        let monitor = self.1.as_ref()?;
        Some(!self.is_closed() && monitor.upgrade().is_some_and(|monitor| monitor.error().is_none()))
    }
}

struct Owner {
    identity: ProcessIdentity,
    stream: Stream,
    stop: Arc<AtomicBool>,
    worker: Thread,
    terminal_error: Arc<OnceLock<String>>,
    done: Arc<AtomicBool>,
    monitor: Option<Weak<crate::xplane::process_events::Monitor>>,
}

struct Slot {
    owner: Weak<Owner>,
    done: Arc<AtomicBool>,
}
static OWNERS: OnceLock<Mutex<BTreeMap<&'static str, Slot>>> = OnceLock::new();

struct Shutdown {
    stream: Stream,
    done: Arc<AtomicBool>,
}
impl Drop for Shutdown {
    fn drop(&mut self) {
        self.stream.close();
        self.done.store(true, Ordering::Release);
    }
}

impl Owner {
    fn start(identity: ProcessIdentity, source: impl Source + 'static) -> Result<Self, String> {
        let monitor = source.process_monitor();
        let clock = format!("assetto:{}", uuid::Uuid::new_v4());
        let stream = Stream::new(1, Frame::Waiting, 0, &clock, LIMITS)
            .map_err(|error| format!("{error:?}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let terminal_error = Arc::new(OnceLock::new());
        let worker_error = terminal_error.clone();
        let shutdown = Shutdown {
            stream: stream.clone(),
            done: done.clone(),
        };
        let cancelled = stop.clone();
        let worker = thread::Builder::new()
            .name("assetto-capture".into())
            .spawn(move || {
                let shutdown = shutdown;
                let mut source = source;
                let origin = Instant::now();
                let mut previous_packet = None;
                let mut previous_sample: Option<Arc<RawSample>> = None;
                let mut previous_error = None;
                while !cancelled.load(Ordering::Acquire) {
                    let began = Instant::now();
                    let frame = match source.poll() {
                        Ok(Some(sample)) => {
                            let packet = sample
                                .physics
                                .get(..4)
                                .and_then(|bytes| bytes.try_into().ok())
                                .map(u32::from_le_bytes);
                            let generation =
                                sample.capture.as_ref().map(|capture| capture.generation);
                            let key = (sample.source.epoch, generation, packet);
                            let changed = previous_packet.replace(key) != Some(key)
                                || previous_sample.as_ref().is_some_and(|previous| {
                                    previous.graphics != sample.graphics
                                        || previous.statics != sample.statics
                                });
                            let recovered = previous_error.take().is_some();
                            (changed || recovered).then(|| {
                                let sample = Arc::new(sample);
                                previous_sample = Some(sample.clone());
                                Frame::Sample(sample)
                            })
                        }
                        Ok(None) => None,
                        Err(error) => {
                            let error: Arc<str> =
                                error.chars().take(1024).collect::<String>().into();
                            let changed = previous_error.as_ref() != Some(&error);
                            previous_error = Some(error.clone());
                            changed.then_some(Frame::Error(error))
                        }
                    };
                    if let Some(frame) = frame {
                        let captured = match &frame {
                            Frame::Sample(sample) => sample
                                .capture
                                .as_ref()
                                .map(|capture| capture.physics.captured_at),
                            _ => None,
                        }
                        .unwrap_or_else(Instant::now);
                        let us =
                            u64::try_from(captured.saturating_duration_since(origin).as_micros())
                                .unwrap_or(u64::MAX);
                        if shutdown
                            .stream
                            .publish(1, frame.clone(), frame, us, &clock)
                            .is_err()
                        {
                            break;
                        }
                    }
                    if let Some(error) = source.terminal_error() {
                        let _ = worker_error.set(error);
                        break;
                    }
                    let period = if previous_error.is_some() {
                        RETRY_PERIOD
                    } else {
                        SAMPLE_PERIOD
                    };
                    thread::park_timeout(period.saturating_sub(began.elapsed()));
                }
                source.close();
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            identity,
            stream,
            stop,
            worker: worker.thread().clone(),
            terminal_error,
            done,
            monitor,
        })
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.unpark();
    }
}

pub(crate) struct SharedSource {
    reader: Option<Reader>,
    latest: bool,
    owner: Arc<Owner>,
}

impl SharedSource {
    pub(crate) fn open(identity: &ProcessIdentity, game: Game) -> Result<Self, String> {
        Self::attach(identity, game, false, || {
            WindowsSource::exact(identity, game)
        })
    }

    pub(crate) fn open_latest(identity: &ProcessIdentity, game: Game) -> Result<Self, String> {
        Self::attach(identity, game, true, || {
            WindowsSource::exact(identity, game)
        })
    }

    fn attach<S: Source + 'static>(
        identity: &ProcessIdentity,
        game: Game,
        latest: bool,
        open: impl FnOnce() -> Result<S, String>,
    ) -> Result<Self, String> {
        let mut registry = OWNERS
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let slot = registry.get(game.id());
        let owner = match slot.filter(|slot| !slot.done.load(Ordering::Acquire)).and_then(|slot| slot.owner.upgrade()) {
            Some(owner) => {
                if owner.identity.pid != identity.pid
                    || owner.identity.epoch != identity.epoch
                    || normalized_path(&owner.identity.path) != normalized_path(&identity.path)
                {
                    return Err("Assetto telemetry still belongs to an earlier process. Retire that reader first.".into());
                }
                owner
            }
            None => {
                if slot.is_some_and(|slot| !slot.done.load(Ordering::Acquire)) {
                    return Err("Assetto is releasing its previous shared-memory reader.".into());
                }
                let owner = Arc::new(Owner::start(identity.clone(), open()?)?);
                registry.insert(
                    game.id(),
                    Slot {
                        owner: Arc::downgrade(&owner),
                        done: owner.done.clone(),
                    },
                );
                owner
            }
        };
        let reader = owner
            .stream
            .subscribe()
            .map_err(|error| format!("{error:?}"))?;
        Ok(Self {
            reader: Some(reader),
            latest,
            owner,
        })
    }
}

impl Source for SharedSource {
    fn terminal_error(&self) -> Option<String> { self.owner.terminal_error.get().cloned() }
    fn poll(&mut self) -> Result<Option<RawSample>, String> {
        let owner = &self.owner;
        if let Some(error) = owner.terminal_error.get() { return Err(error.clone()); }
        if self.reader.is_none() {
            self.reader = Some(
                owner
                    .stream
                    .subscribe()
                    .map_err(|error| format!("{error:?}"))?,
            );
        }
        let limit = if self.latest {
            LIMITS.max_pending_frames + 1
        } else {
            1
        };
        let mut frame = Frame::Waiting;
        for _ in 0..limit {
            let reader = self.reader.as_mut().expect("Source has a reader");
            frame = match reader.try_recv().map_err(|error| format!("{error:?}"))? {
                Some(Delivery::Snapshot(snapshot)) => snapshot.value,
                Some(Delivery::Event(event)) => event.value,
                Some(Delivery::Loss(loss)) => {
                    self.reader = None;
                    return Err(format!(
                        "Assetto telemetry reader lost continuity: {:?}",
                        loss.reason
                    ));
                }
                None => break,
            };
            // Deliver loss/error before processing any recovered sample.
            if matches!(frame, Frame::Error(_)) {
                break;
            }
        }
        match frame {
            Frame::Sample(sample) => Ok(Some((*sample).clone())),
            Frame::Error(error) => Err(error.to_string()),
            Frame::Waiting => Ok(None),
        }
    }

    fn watch(&self, wake: Arc<osg_streams::NativeWake>) -> Result<Option<Watch>, String> {
        self.owner
            .stream
            .watch(wake)
            .map(|watch| Some(Watch(watch, self.owner.monitor.clone())))
            .map_err(|error| format!("{error:?}"))
    }

    // Acquisition recovery belongs to the worker. Consumers release their
    // observation and owner reference when the source is dropped.
    fn close(&mut self) {}
}

#[cfg(test)]
mod tests;
