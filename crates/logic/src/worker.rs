use crate::{
    evaluator::Program,
    resources,
    wire::{self, Reply, Request},
};
use osg_mapping::script::{Invocation, Ticket, Transaction};
use std::{
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant},
};
pub const ARG: &str = "--osg-logic-worker-v1";
const RESPONSE_DEADLINE: Duration = Duration::from_millis(100);
const RESPONSE_TIMEOUT: &str =
    "Script helper response exceeded 100 ms; evaluation is limited to 5 ms.";
pub struct Worker {
    pub source_hash: String,
    child: Option<Child>,
    _resources: resources::Guard,
    outgoing: SyncSender<Request>,
    incoming: Receiver<Result<Reply, String>>,
    pending: Option<(Ticket, Instant)>,
}
impl Worker {
    pub fn spawn(executable: &std::path::Path, source: &str) -> Result<Self, String> {
        let mut command = Command::new(executable);
        command
            .arg(ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let resources = match resources::Guard::attach(&child) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let mut input = child.stdin.take().ok_or("Missing logic worker stdin.")?;
        let mut output = child.stdout.take().ok_or("Missing logic worker stdout.")?;
        let (tx, requests) = mpsc::sync_channel::<Request>(1);
        let (replies, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(request) = requests.recv() {
                if wire::write(&request, &mut input).is_err() {
                    break;
                }
            }
        });
        std::thread::spawn(move || loop {
            let reply = wire::read(&mut output);
            let failed = reply.is_err();
            if replies.send(reply).is_err() || failed {
                break;
            }
        });
        let mut worker = Self {
            source_hash: hash(source),
            child: Some(child),
            _resources: resources,
            outgoing: tx,
            incoming: rx,
            pending: None,
        };
        worker
            .outgoing
            .try_send(Request::Compile {
                source: source.into(),
            })
            .map_err(|e| e.to_string())?;
        match worker
            .incoming
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Script compile helper deadline exceeded.")??
        {
            Reply::Compiled => Ok(worker),
            Reply::Error { message } => Err(message),
            _ => {
                worker.terminate();
                Err("Unexpected script handshake.".into())
            }
        }
    }
    pub fn submit(&mut self, request: Invocation) -> Result<(), String> {
        if self.pending.is_some() || self.child.is_none() {
            return Err("Script helper is busy or stopped.".into());
        }
        let ticket = request.ticket.clone();
        let started = Instant::now();
        self.outgoing
            .try_send(Request::Invoke { request })
            .map_err(|e| e.to_string())?;
        self.pending = Some((ticket, started));
        Ok(())
    }
    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }
    pub fn alive(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }
    pub fn pending(&self) -> Option<&Ticket> {
        self.pending.as_ref().map(|(ticket, _)| ticket)
    }
    pub fn poll(&mut self) -> Option<(Ticket, Result<Transaction, String>)> {
        let (ticket, started) = self.pending.as_ref()?;
        let result = response(&self.incoming, ticket, *started, Instant::now)?;
        let ticket = ticket.clone();
        self.pending = None;
        if result.is_err() {
            self.terminate();
        }
        Some((ticket, result))
    }
    pub fn terminate(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        self.pending = None;
    }
}
// The deadline covers scheduling, transport and decoding, including replies
// already queued when the host resumes. Receiving bytes never extends it.
fn response(
    incoming: &Receiver<Result<Reply, String>>,
    ticket: &Ticket,
    started: Instant,
    mut now: impl FnMut() -> Instant,
) -> Option<Result<Transaction, String>> {
    if now().saturating_duration_since(started) >= RESPONSE_DEADLINE {
        return Some(Err(RESPONSE_TIMEOUT.into()));
    }
    let result = match incoming.try_recv() {
        Ok(Ok(Reply::Completed {
            ticket: reply,
            result,
        })) if reply == *ticket => Some(result),
        Ok(Ok(_)) => Some(Err("Wrong script helper reply token.".into())),
        Ok(Err(error)) => Some(Err(error)),
        Err(TryRecvError::Disconnected) => Some(Err("Script helper terminated.".into())),
        Err(TryRecvError::Empty) => None,
    };
    if now().saturating_duration_since(started) >= RESPONSE_DEADLINE {
        Some(Err(RESPONSE_TIMEOUT.into()))
    } else {
        result
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.terminate();
    }
}
pub fn helper_main() -> Option<i32> {
    if std::env::args().nth(1).as_deref() != Some(ARG) {
        return None;
    }
    Some(
        if serve(&mut std::io::stdin().lock(), &mut std::io::stdout().lock()).is_ok() {
            0
        } else {
            2
        },
    )
}
pub fn serve(
    input: &mut impl std::io::Read,
    output: &mut impl std::io::Write,
) -> Result<(), String> {
    if let Err(message) = resources::child_limits() {
        wire::write(&Reply::Error { message }, output)?;
        return Ok(());
    }
    let Request::Compile { source } = wire::read(input)? else {
        return Err("Compile handshake required.".into());
    };
    let program = match Program::compile(&source) {
        Ok(program) => program,
        Err(message) => {
            wire::write(&Reply::Error { message }, output)?;
            return Ok(());
        }
    };
    wire::write(&Reply::Compiled, output)?;
    while let Ok(request) = wire::read::<Request>(input) {
        let Request::Invoke { request } = request else {
            return Err("Worker source is immutable.".into());
        };
        let result = program
            .call(&request)
            .map_err(|error| error.chars().take(512).collect());
        wire::write(
            &Reply::Completed {
                ticket: request.ticket,
                result,
            },
            output,
        )?;
    }
    Ok(())
}

pub fn hash(source: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(source.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use osg_mapping::engine::{Counter, ProducerToken};

    fn ticket() -> Ticket {
        Ticket {
            producer: ProducerToken {
                session: "00000000-0000-4000-8000-000000000001".into(),
                run_generation: Counter(1),
                node: "script:output".into(),
                sources: vec![],
            },
            epoch: Counter(1),
            sequence: Counter(1),
        }
    }

    fn completed(ticket: &Ticket) -> Reply {
        Reply::Completed {
            ticket: ticket.clone(),
            result: Ok(Transaction {
                state: serde_json::json!({}),
                outputs: vec![],
                timers: vec![],
                logs: vec![],
            }),
        }
    }

    #[test]
    fn expired_queued_reply_cannot_reauthorize_a_completed_transaction() {
        let (sender, incoming) = mpsc::sync_channel(1);
        let ticket = ticket();
        sender.send(Ok(completed(&ticket))).unwrap();
        let started = Instant::now();
        let expired = started + RESPONSE_DEADLINE;
        assert_eq!(
            response(&incoming, &ticket, started, || expired)
                .unwrap()
                .unwrap_err(),
            RESPONSE_TIMEOUT
        );
        // The valid reply was already queued; no late success was consumed.
        assert!(matches!(
            incoming.try_recv().unwrap().unwrap(),
            Reply::Completed { .. }
        ));
    }

    #[test]
    fn deadline_crossing_during_reply_admission_discards_success() {
        let (sender, incoming) = mpsc::sync_channel(1);
        let ticket = ticket();
        sender.send(Ok(completed(&ticket))).unwrap();
        let started = Instant::now();
        let mut clock = [started, started + RESPONSE_DEADLINE].into_iter();
        assert_eq!(
            response(&incoming, &ticket, started, || clock.next().unwrap())
                .unwrap()
                .unwrap_err(),
            RESPONSE_TIMEOUT
        );
        assert!(matches!(incoming.try_recv(), Err(TryRecvError::Empty)));
    }
}
