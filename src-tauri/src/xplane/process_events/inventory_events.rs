//! Ordinary-user process notifications for the registered game catalogue.
use super::*;
use crate::game_activation::ProcessContinuity;
use osg_streams::NativeWake;

const MAX_NAMES: usize = osg_profiles::activation::MAX_GAMES * 2;
const NAMES_PER_QUERY: usize = 16;

pub(crate) struct InventoryEvents {
    stop: Arc<Handle>,
    cancelled: Arc<AtomicBool>,
    worker: std::thread::JoinHandle<()>,
    ready: Arc<ProcessContinuity>,
}

impl InventoryEvents {
    pub(crate) fn start(paths: &[String], wakes: [Arc<NativeWake>; 2]) -> Result<Option<Self>, String> {
        let queries = queries(paths)?;
        if queries.is_empty() {
            return Ok(None);
        }
        let stop = Arc::new(Handle::event()?);
        let cancelled = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(ProcessContinuity::default());
        let worker_ready = ready.clone();
        let worker_stop = stop.clone();
        let worker_cancelled = cancelled.clone();
        let worker = std::thread::Builder::new()
            .name("game-process-inventory-events".into())
            .spawn(move || {
                let _ = run(&queries, &worker_stop, &worker_cancelled, &worker_ready, &wakes);
            })
            .map_err(|error| format!("Cannot start game process notifications: {error}"))?;
        Ok(Some(Self { stop, cancelled, worker, ready }))
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.worker.is_finished()
    }

    pub(crate) fn continuity(&self) -> Option<Arc<ProcessContinuity>> {
        self.ready.is_live().then(|| self.ready.clone())
    }

    pub(crate) fn retire(&self) {
        self.ready.retire();
        self.cancelled.store(true, Ordering::Release);
        self.stop.signal();
    }
}

impl Drop for InventoryEvents {
    fn drop(&mut self) {
        self.retire();
    }
}

fn queries(paths: &[String]) -> Result<Vec<String>, String> {
    let mut names = std::collections::BTreeSet::new();
    for path in paths {
        let name = path.rsplit(['\\', '/']).next().unwrap_or_default();
        if name.is_empty() || name.encode_utf16().count() > 260
            || name.chars().any(|ch| ch == '"' || ch.is_control())
        {
            return Err("A registered executable cannot be watched for process notifications.".into());
        }
        names.insert(name.to_lowercase());
        if names.len() > MAX_NAMES {
            return Err("Too many registered executable names for process notifications.".into());
        }
    }
    let names: Vec<_> = names.into_iter().collect();
    let mut queries = Vec::new();
    for group in names.chunks(NAMES_PER_QUERY) {
        let filter = group.iter().map(|name| format!("ProcessName = \"{name}\""))
            .collect::<Vec<_>>().join(" OR ");
        for class in ["Win32_ProcessStartTrace", "Win32_ProcessStopTrace"] {
            queries.push(format!("SELECT * FROM {class} WHERE {filter}"));
        }
    }
    Ok(queries)
}

struct Withdraw<'a>(&'a ProcessContinuity, &'a [Arc<NativeWake>; 2]);
impl Drop for Withdraw<'_> {
    fn drop(&mut self) {
        self.0.retire();
        for wake in self.1 { wake.notify(); }
    }
}

fn run(queries: &[String], stop: &Handle, cancelled: &AtomicBool, ready: &Arc<ProcessContinuity>, wakes: &[Arc<NativeWake>; 2]) -> Result<(), String> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }
        .map_err(|error| format!("Cannot initialize process monitor COM apartment: {error}"))?;
    let _apartment = Apartment;
    let (sender, receiver) = mpsc::sync_channel(256);
    let inbox = Arc::new(Inbox {
        sender,
        overflow: AtomicBool::new(false),
        scope: OnceLock::new(),
        continuity: Some(ready.clone()),
        wake: Handle::event()?,
    });
    let _subscription = unsafe { subscribe_com(inbox.clone(), cancelled, queries) }?;
    let _withdraw = Withdraw(ready, wakes);
    check_cancelled(cancelled)?;
    drain(&inbox, &receiver)?;
    // Capture an inventory after subscription so starts racing bootstrap are covered.
    if !ready.begin() { return Ok(()); }
    for wake in wakes { wake.notify(); }
    while native::wait_events(stop, &inbox.wake)? {
        drain(&inbox, &receiver)?;
        for wake in wakes { wake.notify(); }
    }
    Ok(())
}

fn drain(inbox: &Inbox, receiver: &Receiver<Event>) -> Result<(), String> {
    // The queue is bounded. Limit each drain even if callbacks keep arriving.
    for _ in 0..256 {
        match receiver.try_recv() {
            Ok(Event::Started(_)) => {}
            Ok(Event::Failed(error)) => return Err(error),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => return Err("Game process notifications disconnected.".into()),
        }
    }
    if inbox.overflow.load(Ordering::Acquire) {
        return Err("The game process notification queue overflowed.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuity_loss_wakes_activation_and_game_context() {
        let live = ProcessContinuity::default();
        assert!(live.begin());
        let wakes = [Arc::new(NativeWake::default()), Arc::new(NativeWake::default())];
        let before = wakes.each_ref().map(|wake| wake.observed());
        drop(Withdraw(&live, &wakes));
        assert!(!live.is_live());
        assert!(wakes.iter().zip(before).all(|(wake, old)| wake.observed() != old));
    }

    #[test]
    fn lifecycle_queries_filter_and_deduplicate_registered_basenames() {
        let paths = [r"C:\Games\Racer.exe".into(), r"D:\Other\racer.exe".into(), r"C:\Games\Pilot's Game.exe".into()];
        let filters = queries(&paths).unwrap();
        assert_eq!(filters.len(), 2);
        for (query, class) in filters.iter().zip(["Win32_ProcessStartTrace", "Win32_ProcessStopTrace"]) {
            assert_eq!(query, &format!("SELECT * FROM {class} WHERE ProcessName = \"pilot's game.exe\" OR ProcessName = \"racer.exe\""));
        }
        assert!(queries(&[r#"C:\Games\invalid"name.exe"#.into()]).is_err());
        let excessive: Vec<_> = (0..=MAX_NAMES).map(|index| format!("game-{index}.exe")).collect();
        assert!(queries(&excessive).is_err());
    }

    #[test]
    fn bootstrap_rejects_already_queued_loss_and_overflow() {
        let (mut inbox, receiver) = test_inbox(2);
        let live = Arc::new(ProcessContinuity::default());
        assert!(live.begin());
        Arc::get_mut(&mut inbox).unwrap().continuity = Some(live.clone());
        inbox.push(Event::Started(10));
        inbox.push(Event::Failed("Process notifications lost.".into()));
        assert!(!live.is_live());
        assert_eq!(drain(&inbox, &receiver).unwrap_err(), "Process notifications lost.");

        let (inbox, receiver) = test_inbox(1);
        inbox.push(Event::Started(10));
        inbox.push(Event::Started(11));
        assert!(drain(&inbox, &receiver).unwrap_err().contains("overflowed"));
    }
}
