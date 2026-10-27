use super::*;
use std::collections::VecDeque;

pub(super) fn persist(storage: &Result<Database,String>, pending: &mut VecDeque<NewActivityEvent>) -> Result<(),String> {
    let storage = storage.as_ref().map_err(Clone::clone)?;
    while !pending.is_empty() {
        let batch = pending.iter().take(512).cloned().collect::<Vec<_>>();
        storage.activity_append(&batch)?;
        pending.drain(..batch.len());
    }
    Ok(())
}
pub(super) fn run(storage: Result<Database,String>, receiver: mpsc::Receiver<Message>, lifecycle: mpsc::Receiver<observations::Batch>, registry:Arc<Mutex<reconcile::Reconciler>>) {
    let mut observe: Option<Observe> = None;
    let mut previous = None;
    let mut pending = VecDeque::new();
    let mut last_prune = std::time::Instant::now();
    let mut last_observation = std::time::Instant::now();
    loop {
        let received = receiver.recv_timeout(Duration::from_millis(750));
        let disconnected = matches!(received,Err(mpsc::RecvTimeoutError::Disconnected));
        let mut acknowledgements = vec![];
        let mut stop = disconnected;
        let mut changed = false;
        for message in received.ok().into_iter().chain(receiver.try_iter().take(255)) {
            match message {
                Message::Event(event) => {
                    changed=true;
                    pending.push_back(event);
                },
                Message::Observe(source) => {let started=std::time::Instant::now();let snapshot=source();pending.extend(registry.lock().unwrap_or_else(|p|p.into_inner()).observe(observations::Batch::events(capture::initial(&snapshot),&snapshot,started)));previous=Some(snapshot);observe=Some(source);},
                Message::Flush(done) => acknowledgements.push(done),
                Message::Stop => stop = true,
            }
        }
        for batch in lifecycle.try_iter(){pending.extend(registry.lock().unwrap_or_else(|p|p.into_inner()).observe(batch));}
        pending.extend(registry.lock().unwrap_or_else(|p|p.into_inner()).ready());
        if changed || !acknowledgements.is_empty() || last_observation.elapsed() >= Duration::from_millis(750) {
            if let Some(source) = &observe {
                let started=std::time::Instant::now();
                let mut snapshot = source();
                if let Some(before)=&previous { snapshot.preserve_unavailable(before); }
                if let Some(before) = previous.replace(snapshot.clone()) {
                    let batch=observations::Batch::poll(&before,&snapshot,started);
                    pending.extend(registry.lock().unwrap_or_else(|p|p.into_inner()).observe(batch));
                }
            }
            last_observation = std::time::Instant::now();
        }
        let outcome = persist(&storage,&mut pending);
        for done in acknowledgements { let _ = done.send(outcome.clone()); }
        if last_prune.elapsed() >= Duration::from_secs(3600) {
            if let Ok(storage) = &storage { let _ = storage.activity_prune(); }
            last_prune = std::time::Instant::now();
        }
        if stop {
            if let Err(error) = outcome { eprintln!("Activity writer stopped with {} unsaved events: {error}",pending.len()); }
            break;
        }
    }
}
