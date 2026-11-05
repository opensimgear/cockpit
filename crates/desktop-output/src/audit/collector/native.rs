use super::*;
use std::{fs::OpenOptions, path::PathBuf, sync::OnceLock};
use windows_sys::Win32::System::Performance::{
    QueryPerformanceCounter, QueryPerformanceFrequency,
};
struct Global {
    engine: Option<Engine>,
    stats: Arc<Stats>,
    configured: bool,
    directory: Option<PathBuf>,
    run: Option<String>,
}
static GLOBAL: OnceLock<Global> = OnceLock::new();
fn qpc() -> Option<u64> {
    let mut value = 0;
    (unsafe { QueryPerformanceCounter(&mut value) } != 0 && value > 0).then_some(value as u64)
}
fn positive(name: &str) -> Result<u64, Fault> {
    let value = std::env::var(name).map_err(|_| Fault::Context)?;
    let parsed = value.parse::<u64>().map_err(|_| Fault::Context)?;
    if parsed == 0 || parsed.to_string() != value {
        return Err(Fault::Context);
    }
    Ok(parsed)
}
pub(crate) fn initialize(role: Role) {
    GLOBAL.get_or_init(|| {
        let stats = Arc::new(Stats::default());
        let directory = std::env::var_os(ENV_DIRECTORY).map(PathBuf::from);
        let run = std::env::var(ENV_RUN).ok();
        let configured = directory.is_some() || std::env::var_os(ENV_RUN).is_some();
        let mut global = Global {
            engine: None,
            stats: stats.clone(),
            configured,
            directory: directory.clone(),
            run: run.clone(),
        };
        if option_env!("OSG_DEV_DESKTOP_INPUT") != Some("1") || !configured {
            return global;
        }
        let result = (|| {
            let directory = directory.ok_or(Fault::Context)?;
            if !directory.is_absolute() || !directory.is_dir() {
                return Err(Fault::Context);
            }
            let run = run.ok_or(Fault::Context)?;
            if !(8..=64).contains(&run.len())
                || !run.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(Fault::Context);
            }
            let mut frequency = 0;
            if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
                return Err(Fault::Clock);
            }
            let instance = qpc().ok_or(Fault::Clock)?;
            let (parent_pid, parent_instance, channel) = match role {
                Role::Parent => {
                    if [ENV_PARENT_PID, ENV_PARENT_INSTANCE, ENV_CHANNEL]
                        .iter()
                        .any(|name| std::env::var_os(name).is_some())
                    {
                        return Err(Fault::Context);
                    }
                    (None, None, None)
                }
                Role::Worker => {
                    let pid = positive(ENV_PARENT_PID)?;
                    if pid > u32::MAX as u64 || pid == std::process::id() as u64 {
                        return Err(Fault::Context);
                    }
                    (
                        Some(pid),
                        Some(positive(ENV_PARENT_INSTANCE)?),
                        Some(positive(ENV_CHANNEL)?),
                    )
                }
            };
            let context = Context {
                run: run.clone(),
                role,
                pid: std::process::id() as u64,
                instance,
                parent_pid,
                parent_instance,
                channel,
            };
            let file = directory.join(format!("audit-{run}-{}-{instance}.jsonl", context.pid));
            let writer = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(file)
                .map_err(|_| Fault::Io)?;
            Engine::new(
                writer,
                context,
                frequency as u64,
                Arc::new(qpc),
                Limits::default(),
                stats.clone(),
            )
        })();
        match result {
            Ok(engine) => global.engine = Some(engine),
            Err(error) => stats.fault(error),
        }
        global
    });
}
fn engine() -> Option<&'static Engine> {
    GLOBAL.get().and_then(|state| state.engine.as_ref())
}
pub(crate) fn enabled() -> bool {
    engine().is_some()
}
pub(crate) fn stamp() -> Option<Stamp> {
    engine()?.stamp()
}
pub(crate) fn next_id() -> Option<Id> {
    engine()?.next_id()
}
pub(crate) fn alias(kind: AliasKind, key: &[u8]) -> Option<Id> {
    engine()?.alias(kind, key)
}
pub(crate) fn record_at(at: Stamp, kind: Kind, fields: &[u64]) {
    if let Some(engine) = engine() {
        engine.record_at(at, kind, fields);
    }
}
pub(crate) fn fault(reason: Fault) {
    if let Some(state) = GLOBAL.get() {
        state.stats.fault(reason);
    }
}
pub(crate) fn process_instance() -> Option<Id> {
    Some(engine()?.context.instance)
}
pub(crate) fn parent_instance() -> Option<Id> {
    engine()?.context.parent_instance
}
pub(crate) fn parent_pid() -> Option<u64> {
    engine()?.context.parent_pid
}
pub(crate) fn channel() -> Option<Id> {
    engine()?.context.channel
}
pub(crate) fn configure_child(command: &mut Command, channel: Id) {
    if let (Some(engine), Some(state)) = (engine(), GLOBAL.get()) {
        if channel == 0 {
            state.stats.fault(Fault::Context);
            return;
        }
        if let (Some(directory), Some(run)) = (&state.directory, &state.run) {
            command
                .env(ENV_DIRECTORY, directory)
                .env(ENV_RUN, run)
                .env(ENV_PARENT_PID, engine.context.pid.to_string())
                .env(ENV_PARENT_INSTANCE, engine.context.instance.to_string())
                .env(ENV_CHANNEL, channel.to_string());
        }
    }
}
fn unavailable() -> FlushReport {
    GLOBAL
        .get()
        .map(|state| FlushReport {
            configured: state.configured,
            counters: state.stats.snapshot(),
            ..FlushReport::default()
        })
        .unwrap_or_default()
}
pub(crate) fn checkpoint(reason: Checkpoint, timeout: Duration) -> FlushReport {
    engine()
        .map(|engine| {
            engine.barrier(
                false,
                match reason {
                    Checkpoint::Stopped => "stopped",
                    Checkpoint::Manual => "manual",
                    Checkpoint::RunComplete => "run-complete",
                },
                timeout,
            )
        })
        .unwrap_or_else(unavailable)
}
pub(crate) fn finish(reason: ExitReason, timeout: Duration) -> FlushReport {
    engine()
        .map(|engine| {
            engine.barrier(
                true,
                match reason {
                    ExitReason::Normal => "normal",
                    ExitReason::Stop => "stop",
                    ExitReason::Error => "error",
                },
                timeout,
            )
        })
        .unwrap_or_else(unavailable)
}
