use crate::{
    resources,
    typed::{Definition, Invocation, Reply, Request},
    typed_evaluator::Program,
    wire,
};
use osg_mapping::{
    engine::typed::{ScriptInvocation, ScriptTicket, ScriptTransaction},
    program::Node,
};
use std::{
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant},
};
pub const ARG: &str = "--osg-logic-worker-typed-v2";
const RESPONSE_DEADLINE: Duration = Duration::from_millis(100);
const RESPONSE_TIMEOUT: &str =
    "Script helper response exceeded 100 ms; evaluation is limited to 5 ms.";
pub struct TypedWorker {
    definition: Definition,
    pub source_hash: String,
    child: Option<Child>,
    _resources: resources::Guard,
    outgoing: SyncSender<Request>,
    incoming: Receiver<Result<Reply, String>>,
    pending: Option<(Invocation, Instant)>,
}
impl TypedWorker {
    pub fn spawn(executable: &std::path::Path, node: &Node) -> Result<Self, String> {
        let definition = Definition::from_node(node)?;
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
            source_hash: crate::worker::hash(&definition.source),
            definition: definition.clone(),
            child: Some(child),
            _resources: resources,
            outgoing: tx,
            incoming: rx,
            pending: None,
        };
        worker
            .outgoing
            .try_send(Request::Compile { definition })
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
    pub fn submit(&mut self, request: ScriptInvocation) -> Result<(), String> {
        if self.pending.is_some() || self.child.is_none() {
            return Err("Script helper is busy or stopped.".into());
        }
        let request = Invocation::from(request);
        if let Err(error) = self.definition.validate_invocation(&request) {
            self.terminate();
            return Err(error);
        }
        let retained = request.clone();
        let started = Instant::now();
        if let Err(error) = self.outgoing.try_send(Request::Invoke { request }) {
            self.terminate();
            return Err(error.to_string());
        }
        self.pending = Some((retained, started));
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
    pub fn pending(&self) -> Option<&ScriptTicket> {
        self.pending.as_ref().map(|(request, _)| &request.ticket)
    }
    pub fn poll(&mut self) -> Option<(ScriptTicket, Result<ScriptTransaction, String>)> {
        let (request, started) = self.pending.as_ref()?;
        let result = response(
            &self.incoming,
            request,
            &self.definition,
            *started,
            Instant::now,
        )?;
        let ticket = request.ticket.clone();
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
    request: &Invocation,
    definition: &Definition,
    started: Instant,
    mut now: impl FnMut() -> Instant,
) -> Option<Result<ScriptTransaction, String>> {
    if now().saturating_duration_since(started) >= RESPONSE_DEADLINE {
        return Some(Err(RESPONSE_TIMEOUT.into()));
    }
    let result = match incoming.try_recv() {
        Ok(Ok(Reply::Completed {
            ticket: reply,
            result,
        })) if reply == request.ticket => Some(result.and_then(|transaction| {
            definition.validate_transaction(request, &transaction)?;
            Ok(transaction)
        })),
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

impl Drop for TypedWorker {
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
    let Request::Compile { definition } = wire::read(input)? else {
        return Err("Compile handshake required.".into());
    };
    let program = match Program::compile(definition) {
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

#[cfg(test)]
#[path = "typed_worker_tests.rs"]
mod tests;
