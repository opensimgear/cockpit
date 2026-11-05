use super::*;
use std::collections::{BTreeMap, VecDeque};

pub(super) fn persist(storage: &Result<Database,String>, pending: &mut VecDeque<NewActivityEvent>) -> Result<(),String> {
    let storage = storage.as_ref().map_err(Clone::clone)?;
    while !pending.is_empty() {
        let batch = pending.iter().take(512).cloned().collect::<Vec<_>>();
        storage.activity_append(&batch)?;
        pending.drain(..batch.len());
    }
    Ok(())
}
pub(super) fn run(storage: Result<Database,String>, receiver: mpsc::Receiver<Message>, lifecycle: mpsc::Receiver<observations::Batch>, registry:Arc<Mutex<reconcile::Reconciler>>, revision: &revision::Revision, wake: Arc<osg_streams::NativeWake>) {
    let _ = wake.bind_thread(std::thread::current());
    let mut sources = BTreeMap::new();
    let mut pending = VecDeque::new();
    let mut last_prune = std::time::Instant::now();
    let mut retry_due: Option<std::time::Instant> = None;
    let mut persistence_error = None;
    loop {
        // Observe the epoch before draining both queues. A producer racing the
        // drain advances it, so wait_scheduled refuses to sleep past that work.
        let observed = wake.observed();
        let mut acknowledgements = vec![];
        let mut stop = false;
        let mut changed = false;
        let mut changed_groups = 0;
        let mut drained = 0;
        while drained < 256 {
            let message = match receiver.try_recv() {
                Ok(message) => message,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => { stop = true; break; }
            };
            drained += 1;
            match message {
                Message::Event(event) => {
                    changed=true;
                    if let Some(group) = metadata::Group::for_event(&event.event_type) { changed_groups |= group.bit(); }
                    pending.push_back(event);
                },
                Message::Observe(mut source) => {
                    if let Some(batch) = source.take(true) {
                        pending.extend(registry.lock().unwrap_or_else(|p|p.into_inner()).observe(batch));
                    }
                    sources.insert(source.group, source);
                },
                Message::Flush(done) => acknowledgements.push(done),
                Message::Stop => stop = true,
            }
        }
        for batch in lifecycle.try_iter(){pending.extend(registry.lock().unwrap_or_else(|p|p.into_inner()).observe(batch));}
        pending.extend(registry.lock().unwrap_or_else(|p|p.into_inner()).ready());
        for source in sources.values_mut() {
            let force = !acknowledgements.is_empty() || source.group.map_or(changed, |group| changed_groups & group.bit() != 0);
            if let Some(batch) = source.take(force) {
                pending.extend(registry.lock().unwrap_or_else(|p|p.into_inner()).observe(batch));
            }
        }
        let queued = pending.len();
        let now = std::time::Instant::now();
        let outcome = if stop || !acknowledgements.is_empty() || retry_due.is_none_or(|due| now >= due) {
            let outcome = persist(&storage,&mut pending);
            persistence_error = outcome.as_ref().err().cloned();
            // Retry only outstanding persistence. This deadline never captures
            // metadata and cannot turn into a replacement acquisition timer.
            retry_due = (outcome.is_err() && !pending.is_empty()).then(|| now + Duration::from_millis(750));
            outcome
        } else { Err(persistence_error.clone().expect("scheduled persistence retry has an error")) };
        if pending.len() < queued { revision.changed(); }
        for done in acknowledgements { let _ = done.send(outcome.clone()); }
        if last_prune.elapsed() >= Duration::from_secs(3600) {
            if let Ok(storage) = &storage {
                if storage.activity_prune().is_ok_and(|count| count > 0) { revision.changed(); }
            }
            last_prune = std::time::Instant::now();
        }
        if stop {
            if let Err(error) = outcome { eprintln!("Activity writer stopped with {} unsaved events: {error}",pending.len()); }
            break;
        }
        // Do not park when the bounded main-channel drain may have left work.
        if drained < 256 {
            let now=std::time::Instant::now();
            let prune_due=last_prune+Duration::from_secs(3600);
            let deadline=retry_due.map_or(prune_due, |due| due.min(prune_due));
            wake.wait_scheduled(observed,Some(deadline.saturating_duration_since(now)));
        }
    }
}
