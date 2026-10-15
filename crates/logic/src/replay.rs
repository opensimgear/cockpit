//! Replay validates code results and recorded scheduling independently of wall-clock execution speed.
use crate::worker::Worker;
use osg_mapping::{
    runtime::Recorded,
    script::Transaction,
    trace::{Step, Trace},
};
use std::{collections::BTreeMap, path::Path, time::Duration};
pub fn replay(trace: &Trace, helper: &Path) -> Result<Vec<Recorded>, String> {
    trace.validate_fingerprints()?;
    let mut workers = BTreeMap::new();
    let mut results = BTreeMap::<String, Result<Transaction, String>>::new();
    for script in &trace.profile.scripts {
        workers.insert(script.id.clone(), Worker::spawn(helper, &script.source)?);
    }
    for step in &trace.steps {
        match step {
            Step::Dispatch { requests } => {
                for (id, request) in requests {
                    let worker = workers.get_mut(id).ok_or("Unknown replay script.")?;
                    worker.submit(request.clone())?;
                    loop {
                        if let Some((_, result)) = worker.poll() {
                            results.insert(id.clone(), result);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }
            Step::ScriptReply {
                id, transaction, ..
            } => {
                let actual = results.remove(id).ok_or("Missing replay callback.")??;
                if serde_json::to_value(actual).unwrap()
                    != serde_json::to_value(transaction).unwrap()
                {
                    return Err("Replayed callback produced a different transaction.".into());
                }
            }
            Step::ScriptFault { id, .. } => {
                results.remove(id);
            }
            Step::Replace { .. } if !trace.profile.scripts.is_empty() => {
                return Err("Export a separate trace for each scripted run.".into())
            }
            _ => {}
        }
    }
    trace.replay()
}
