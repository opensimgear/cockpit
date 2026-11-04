use super::{usb::terminal_failure, usb_source::Clock, *};
use crate::stm32_panel_capture::{Batch, Ready};
use osg_streams::{Delivery, LossReason};

const FRESHNESS: Duration = Duration::from_millis(250);

fn verify_route(target: &Target, sample: &Ready) -> Result<(), String> {
    let Target::Can {
        epoch,
        node,
        lease,
        boot,
        configuration,
        stream,
        ..
    } = target
    else {
        return Err("Expected a CAN panel route.".into());
    };
    let route = &sample.route;
    if epoch.as_u64() != Some(route.gateway_epoch.into())
        || node.as_u64() != Some(route.node.into())
        || lease.as_u64() != Some(route.lease.into())
        || boot.as_u64() != Some(route.boot_session.into())
        || configuration.as_u64() != Some(route.configuration_generation.into())
        || stream.as_u64() != Some(route.configuration_generation.into())
    {
        return Err("The native CAN panel route changed.".into());
    }
    Ok(())
}

fn bit(bytes: &[u8], index: usize) -> bool {
    bytes[index / 8] & (1 << (index % 8)) != 0
}

fn state_changes(
    source: &Source,
    sample: &Ready,
    axes: usize,
    baseline: bool,
) -> Result<Vec<Change>, String> {
    let layout = &sample.layout;
    let buttons = usize::from(layout.contact_count)
        + 2 * usize::from(layout.encoder_count)
        + 3 * usize::from(layout.selector_count);
    let held: Vec<_> = (0..buttons)
        .map(|index| bit(&sample.state.held, index))
        .collect();
    let available: Vec<_> = (0..axes)
        .map(|index| sample.state.axis_available & (1 << index) != 0)
        .collect();
    source.state_changes(
        &json!({
            "held": held, "axes": &sample.state.axes[..axes], "axisAvailable": available,
            "sampledAtMs": sample.state_sampled_at_ms,
        }),
        baseline,
        true,
    )
}

fn baseline(
    source: &mut Source,
    sample: &Ready,
    axes: usize,
    clock: Clock,
    now: Instant,
) -> Result<Frame, String> {
    verify_route(&source.spec.target, sample)?;
    Clock::fresh(sample.received_at, now)?;
    let changes = state_changes(source, sample, axes, true)?;
    let frame = source.frame(
        clock
            .at(sample.received_at.max(clock.started))?
            .max(source.at),
        Reason::Baseline,
        changes,
    )?;
    source.cursor = Some((sample.route.configuration_generation, sample.cursor));
    source.sampled_at = Some(sample.state_sampled_at_ms);
    Ok(frame)
}

fn changes(
    source: &mut Source,
    batch: &Batch,
    axes: usize,
    clock: Clock,
    now: Instant,
) -> Result<Vec<Frame>, String> {
    let sample = &batch.latest;
    verify_route(&source.spec.target, sample)?;
    Clock::fresh(batch.received_at, now)?;
    let (_, cursor) = source.cursor.ok_or("CAN panel input needs a baseline.")?;
    if sample.cursor.checked_sub(cursor) != Some(batch.edges.len() as u32) {
        return Err("CAN panel cursor changed unexpectedly.".into());
    }
    if batch.edges.iter().any(|edge| {
        Duration::from_millis(u64::from(
            sample.sampled_at_ms.wrapping_sub(edge.captured_at_ms),
        )) >= FRESHNESS
    }) {
        return Err("A CAN panel edge expired before admission.".into());
    }
    let at = clock.at(batch.received_at)?.max(source.at);
    let mut frames = Vec::new();
    for edge in &batch.edges {
        let state_port = format!("button:{}", edge.control);
        let event_port = format!("button-event:{}", edge.control);
        let values = source
            .spec
            .ports
            .iter()
            .filter(|port| port.observed == state_port || port.observed == event_port)
            .map(|port| Change {
                port_id: port.logical.clone(),
                sample: SignalSample::scalar(PortValue::Boolean(edge.pressed)),
            })
            .collect::<Vec<_>>();
        if !values.is_empty() {
            frames.push(source.frame(at, Reason::Change, values)?);
        }
    }
    if batch.edges.is_empty() && source.sampled_at != Some(sample.state_sampled_at_ms) {
        let values = state_changes(source, sample, axes, false)?;
        if !values.is_empty() {
            frames.push(source.frame(at, Reason::Change, values)?);
        }
        source.sampled_at = Some(sample.state_sampled_at_ms);
    }
    source.cursor = Some((sample.route.configuration_generation, sample.cursor));
    Ok(frames)
}

pub(super) fn run(
    spec: Spec,
    io: Arc<dyn Io>,
    mailbox: Arc<Mutex<Mailbox>>,
    revoked: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    admitted: Arc<AtomicBool>,
    started: Instant,
    origin: u64,
    wake: Arc<NativeWake>,
) {
    let logical = spec.logical.clone();
    let mut checkpoint = None;
    let outcome = (|| -> Result<(), String> {
        let Target::Can {
            gateway, module, ..
        } = &spec.target
        else {
            unreachable!()
        };
        let capture = io.can_capture(gateway, module)?;
        let snapshot = io.can_cached(gateway)?;
        let axes = snapshot["modules"]
            .as_array()
            .and_then(|modules| modules.iter().find(|entry| entry["id"] == *module))
            .and_then(|entry| entry["axisCount"].as_u64())
            .filter(|count| *count <= 8)
            .ok_or("The CAN panel axis profile is unavailable.")? as usize;
        let _ = wake.bind_thread(thread::current());
        let _watch = capture.watch(wake.clone())?;
        let mut reader = capture.subscribe()?;
        let clock = Clock { started, origin };
        let mut source = Source {
            spec,
            cursor: None,
            held: vec![],
            sequence: 0,
            at: 0,
            retry: started,
            generation: 1,
            sampled_at: None,
        };
        let mut running = false;
        let mut loss_epoch = 0;
        loop {
            let observed = wake.observed();
            let now = Instant::now();
            capture.expire(now);
            let mut state = mailbox.lock().unwrap_or_else(|p| p.into_inner());
            if revoked.load(Ordering::Acquire) || state.blocked.contains(&logical) {
                return Ok(());
            }
            let active_now = active.load(Ordering::Acquire);
            let delivering = active_now && admitted.load(Ordering::Acquire);
            if active_now != running || state.loss_epoch != loss_epoch {
                running = active_now;
                loss_epoch = state.loss_epoch;
                reader = capture.subscribe()?;
                source.cursor = None;
                source.generation = source
                    .generation
                    .checked_add(1)
                    .ok_or("CAN input generation exhausted.")?;
            }
            let Some(delivery) = reader
                .try_recv()
                .map_err(|error| format!("CAN panel subscription failed: {error:?}"))?
            else {
                drop(state);
                wake.wait_scheduled(
                    observed,
                    capture
                        .deadline()
                        .map(|deadline| deadline.saturating_duration_since(Instant::now())),
                );
                continue;
            };
            let now = Instant::now();
            let result = match delivery {
                Delivery::Snapshot(snapshot) => snapshot
                    .value
                    .0
                    .as_ref()
                    .ok_or_else(|| "Waiting for a fresh CAN panel baseline.".into())
                    .and_then(|sample| {
                        baseline(&mut source, sample, axes, clock, now)
                            .map(|frame| (Some(frame), vec![], sample.received_at))
                    }),
                Delivery::Event(event) => {
                    let batch = &event.value;
                    if !delivering || source.cursor.is_none() {
                        baseline(&mut source, &batch.latest, axes, clock, now)
                            .map(|frame| (Some(frame), vec![], batch.latest.received_at))
                    } else {
                        changes(&mut source, batch, axes, clock, now)
                            .map(|frames| (None, frames, batch.latest.received_at))
                    }
                }
                Delivery::Loss(loss) => {
                    if loss.reason == LossReason::Closed {
                        return Err("CAN panel capture closed.".into());
                    }
                    if loss.reason == LossReason::Overflow {
                        reader = capture.subscribe()?;
                    }
                    Err("CAN panel continuity was lost.".into())
                }
            };
            match result {
                Ok((baseline, frames, witnessed)) => {
                    state.diagnostics.remove(&logical);
                    state.liveness.insert(
                        logical.clone(),
                        (
                            source.generation,
                            source.sequence,
                            clock.at(witnessed.max(clock.started))?,
                        ),
                    );
                    if let Some(frame) = baseline {
                        state.baselines.insert(logical.clone(), frame.clone());
                        if delivering {
                            if state.frames.len() == MAX_QUEUE {
                                state.overflow(&logical)?;
                            } else {
                                state.frames.push_back(frame);
                            }
                        }
                    }
                    if delivering {
                        if state.frames.len() + frames.len() > MAX_QUEUE {
                            state.overflow(&logical)?;
                        } else {
                            state.frames.extend(frames);
                        }
                    }
                    checkpoint = Some((source.generation, source.sequence, source.at));
                    state.notify();
                }
                Err(error) => {
                    if source.cursor.take().is_some() {
                        terminal_failure(
                            &mut state,
                            &logical,
                            &error,
                            if delivering { checkpoint.take() } else { None },
                        );
                        source.generation = source
                            .generation
                            .checked_add(1)
                            .ok_or("CAN input generation exhausted.")?;
                    } else {
                        state.diagnostics.insert(logical.clone(), error);
                        state.notify();
                    }
                }
            }
        }
    })();
    if let Err(error) = outcome {
        if !revoked.load(Ordering::Acquire) {
            terminal_failure(
                &mut mailbox.lock().unwrap_or_else(|p| p.into_inner()),
                &logical,
                &error,
                checkpoint,
            );
        }
    }
}
