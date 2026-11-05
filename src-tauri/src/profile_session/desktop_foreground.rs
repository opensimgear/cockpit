use osg_desktop_output::{windows::focus::ForegroundSnapshot, ProcessIdentity};
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct Cache {
    watch_id: u64,
    sequence: u64,
    identity: Option<ProcessIdentity>,
    error: Option<String>,
    terminal: bool,
}

impl Cache {
    fn observe(&mut self, snapshot: ForegroundSnapshot, own_executable: Option<&str>) {
        let current = (self.watch_id, self.sequence);
        let incoming = (snapshot.watch_id, snapshot.sequence);
        let terminal_same_sequence = incoming == current && snapshot.terminal && !self.terminal;
        if incoming < current || (incoming == current && !terminal_same_sequence) {
            return;
        }
        self.watch_id = snapshot.watch_id;
        self.sequence = snapshot.sequence;
        self.error = snapshot.error;
        self.terminal = snapshot.terminal;
        if let Some(identity) = snapshot.identity {
            if !own_executable.is_some_and(|path| path.eq_ignore_ascii_case(&identity.executable)) {
                self.identity = Some(identity);
            }
        }
    }

    fn recent_application(&self) -> Result<ProcessIdentity, String> {
        if let Some(error) = &self.error {
            return Err(format!("Recent application observer unavailable: {error}"));
        }
        self.identity.clone().ok_or_else(|| {
            "Focus the application you want to control, then return to Cockpit and choose it."
                .into()
        })
    }
}

static RECENT: OnceLock<Mutex<Cache>> = OnceLock::new();
fn cache() -> &'static Mutex<Cache> {
    RECENT.get_or_init(|| Mutex::new(Cache::default()))
}

fn observe(snapshot: ForegroundSnapshot, own_executable: Option<&str>) {
    cache()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .observe(snapshot, own_executable);
}

fn process_snapshot(
    snapshot: ForegroundSnapshot,
    own_executable: Option<&str>,
    wake: &osg_streams::NativeWake,
    update: impl FnOnce(ForegroundSnapshot, Option<&str>),
) {
    let terminal = snapshot.terminal;
    update(snapshot, own_executable);
    if terminal {
        wake.notify();
    }
}

pub(crate) fn recent_application() -> Result<ProcessIdentity, String> {
    cache()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .recent_application()
}

#[cfg(windows)]
pub(crate) fn start(
    wake: std::sync::Arc<osg_streams::NativeWake>,
) -> Option<osg_desktop_output::windows::focus::ForegroundWatch> {
    use osg_desktop_output::windows::focus::ForegroundWatch;
    let own_executable = match std::env::current_exe() {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(error) => {
            cache()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .error = Some(format!("Cannot identify Cockpit executable: {error}"));
            return None;
        }
    };
    let own_for_wake = own_executable.clone();
    let terminal_wake = wake.clone();
    let watch = match ForegroundWatch::open(std::sync::Arc::new(move |snapshot| {
        process_snapshot(snapshot, Some(&own_for_wake), &terminal_wake, observe);
    })) {
        Ok(watch) => watch,
        Err(error) => {
            cache()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .error = Some(error);
            return None;
        }
    };
    match watch.snapshot() {
        Ok(snapshot) => observe(snapshot, Some(&own_executable)),
        Err(error) => {
            cache()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .error = Some(error);
            return None;
        }
    }
    Some(watch)
}

#[cfg(not(windows))]
pub(crate) fn start(_wake: std::sync::Arc<osg_streams::NativeWake>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_observer_starts_seeds_the_cache_and_retires_without_input() {
        let watch = start(std::sync::Arc::new(osg_streams::NativeWake::default()))
            .expect("The native foreground observer must register without an active profile");
        let snapshot = watch.snapshot().unwrap();
        assert!(snapshot.watch_id > 0 && snapshot.sequence > 0);
        if let Ok(identity) = recent_application() {
            assert!(
                !identity
                    .executable
                    .eq_ignore_ascii_case(&std::env::current_exe().unwrap().to_string_lossy()),
                "Returning to Cockpit must preserve the external application selection"
            );
        }
        drop(watch);
    }

    fn snapshot(
        watch_id: u64,
        sequence: u64,
        identity: Option<ProcessIdentity>,
        error: Option<&str>,
    ) -> ForegroundSnapshot {
        ForegroundSnapshot {
            watch_id,
            sequence,
            identity,
            error: error.map(str::to_owned),
            terminal: false,
        }
    }
    fn identity(pid: u32, executable: &str) -> ProcessIdentity {
        ProcessIdentity {
            executable: executable.into(),
            pid,
            created: format!("created-{pid}"),
        }
    }

    #[test]
    fn cache_fences_old_initial_seed_after_newer_event_and_skips_own_executable() {
        let mut cache = Cache::default();
        cache.observe(
            snapshot(10, 2, Some(identity(11, "C:\\Games\\target.exe")), None),
            Some("C:\\Cockpit.exe"),
        );
        cache.observe(
            snapshot(10, 1, Some(identity(10, "C:\\Games\\old.exe")), None),
            Some("C:\\Cockpit.exe"),
        );
        cache.observe(
            snapshot(10, 3, Some(identity(1, "c:\\cockpit.EXE")), None),
            Some("C:\\Cockpit.exe"),
        );
        assert_eq!(
            cache.recent_application().unwrap(),
            identity(11, "C:\\Games\\target.exe")
        );
    }

    #[test]
    fn observer_error_is_reported_even_when_a_recent_selection_is_cached() {
        let mut cache = Cache::default();
        cache.observe(
            snapshot(20, 1, Some(identity(12, "C:\\Games\\target.exe")), None),
            Some("C:\\Cockpit.exe"),
        );
        cache.observe(
            snapshot(20, 2, None, Some("native event hook failed")),
            Some("C:\\Cockpit.exe"),
        );
        assert!(cache
            .recent_application()
            .unwrap_err()
            .contains("native event hook failed"));
    }

    #[test]
    fn terminal_snapshot_at_same_sequence_is_not_fenced_as_stale() {
        let mut cache = Cache::default();
        cache.observe(
            snapshot(
                30,
                u64::MAX,
                Some(identity(13, "C:\\Games\\target.exe")),
                None,
            ),
            Some("C:\\Cockpit.exe"),
        );
        let mut terminal = snapshot(30, u64::MAX, None, Some("foreground sequence exhausted"));
        terminal.terminal = true;
        cache.observe(terminal, Some("C:\\Cockpit.exe"));
        assert!(cache
            .recent_application()
            .unwrap_err()
            .contains("sequence exhausted"));
        assert!(cache.terminal);
    }

    #[test]
    fn terminal_snapshot_updates_cache_then_notifies_the_profile_worker() {
        let mut cache = Cache::default();
        let wake = osg_streams::NativeWake::default();
        process_snapshot(
            snapshot(40, 1, Some(identity(14, "C:\\Games\\target.exe")), None),
            Some("C:\\Cockpit.exe"),
            &wake,
            |event, own| cache.observe(event, own),
        );
        assert_eq!(
            wake.observed(),
            0,
            "ordinary foreground changes do not wake profile polling"
        );
        let mut terminal = snapshot(40, 1, None, Some("native owner stopped"));
        terminal.terminal = true;
        process_snapshot(terminal, Some("C:\\Cockpit.exe"), &wake, |event, own| {
            cache.observe(event, own);
        });
        assert_eq!(wake.observed(), 1);
        assert!(cache
            .recent_application()
            .unwrap_err()
            .contains("native owner stopped"));
    }
}
