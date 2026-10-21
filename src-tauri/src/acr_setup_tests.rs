use super::*;
use std::{collections::BTreeMap, path::PathBuf};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("osg-acr-setup-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn executable(&self) -> PathBuf {
        let path = self.0.join("acr.exe");
        fs::write(&path, b"OpenSimGear authored executable identity fixture\n").unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn identity() -> ProcessIdentity {
    ProcessIdentity {
        pid: 700,
        path: "C:/Games/ACR/acr.exe".into(),
        epoch: 77,
    }
}
fn write_text(bytes: &mut [u8], offset: usize, count: usize, text: &str) {
    bytes[offset..offset + count * 2].fill(0);
    for (index, c) in text.encode_utf16().take(count - 1).enumerate() {
        bytes[offset + index * 2..offset + index * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
}
fn sample(packet: u32, status: i32) -> RawSample {
    let mut s = RawSample {
        source: SourceIdentity {
            id: "acr-pid-700".into(),
            epoch: 77,
        },
        physics: include_bytes!("../../tests/fixtures/acr/live.physics.bin").to_vec(),
        graphics: include_bytes!("../../tests/fixtures/acr/live.graphics.bin").to_vec(),
        statics: include_bytes!("../../tests/fixtures/acr/live.static.bin").to_vec(),
    };
    s.physics[0..4].copy_from_slice(&packet.to_le_bytes());
    s.graphics[4..8].copy_from_slice(&status.to_le_bytes());
    write_text(&mut s.statics, 0, 15, "1");
    write_text(&mut s.statics, 30, 15, "0.3.0");
    write_text(&mut s.statics, 68, 33, "citroen_xsara");
    write_text(&mut s.statics, 134, 33, "stage_finland");
    s
}
#[test]
fn fingerprint_is_exact_bounded_durable_and_changes_after_game_update() {
    let scratch = Scratch::new();
    let path = scratch.executable();
    let first = fingerprint(&path).unwrap();
    assert!(first["fingerprint"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(first["sha256"].as_str().unwrap().len(), 64);
    assert!(first.get("pid").is_none());
    assert_eq!(
        fingerprint(&path).unwrap()["fingerprint"],
        first["fingerprint"]
    );
    fs::write(&path, b"updated executable fixture").unwrap();
    assert_ne!(
        fingerprint(&path).unwrap()["fingerprint"],
        first["fingerprint"]
    );
    fs::write(&path, []).unwrap();
    assert!(fingerprint(&path).unwrap_err().contains("empty"));
    fs::File::create(&path)
        .unwrap()
        .set_len(MAX_EXECUTABLE + 1)
        .unwrap();
    assert!(fingerprint(&path).is_err());
    assert!(fingerprint(Path::new("acr.exe")).is_err());
    assert!(fingerprint(&scratch.0.join("acs.exe")).is_err());
}
#[test]
fn known_assetto_producers_are_filtered_before_process_queries() {
    for name in [
        "acr.exe",
        "ACR.exe",
        "ACS.exe",
        "acs_x86.exe",
        "AC2-Win64-Shipping.exe",
        "AssettoCorsaEVO.exe",
    ] {
        assert!(is_competing_image(name), "{name}");
    }
    for name in [
        "AssettoCorsa.exe",
        "C:/Game/AC2.exe",
        "Cockpit.exe",
        "explorer.exe",
        "DCS.exe",
        "acr-not-a-game.exe",
        "C:/acr.exe/helper.exe",
    ] {
        assert!(!is_competing_image(name), "{name}");
    }
    assert!(paths_match(
        "\\\\?\\C:\\Games\\ACR\\acr.exe",
        "c:/games/acr/acr.exe"
    ));
    assert!(!paths_match(
        "C:/Games/ACR/acr.exe",
        "C:/Games/Other/acr.exe"
    ));
    assert!(conflict_message(&[json!({"pid":32,"image":"acs.exe"})]).contains("PID 32"));
}
#[test]
fn baseline_then_advancing_live_packets_validate_required_channels_without_native_claim() {
    let mut verifier = Verifier::new(&identity()).unwrap();
    assert!(!verifier.observe(&sample(10, 2), 0).unwrap().fresh);
    let first = verifier.assessment(0);
    assert_eq!(first["compatible"], false);
    assert_eq!(first["status"], "waiting");
    let fresh = verifier.observe(&sample(11, 2), 100).unwrap();
    assert!(required_channels_valid(&fresh));
    let assessed = verifier.assessment(100);
    assert_eq!(assessed["compatible"], true);
    assert_eq!(assessed["status"], "receiving");
    assert!(assessed.get("nativeVerified").is_none());
    assert!(assessed["context"]["gameVersion"].as_str().is_some());
    assert_eq!(assessed["maxRpmAvailable"], true);
}
#[test]
fn optional_maximum_does_not_block_rpm_gear_or_speed_and_never_infers_redline() {
    let mut verifier = Verifier::new(&identity()).unwrap();
    let mut s = sample(10, 2);
    s.statics[412..416].copy_from_slice(&0i32.to_le_bytes());
    verifier.observe(&s, 0).unwrap();
    s.physics[..4].copy_from_slice(&11u32.to_le_bytes());
    verifier.observe(&s, 100).unwrap();
    let assessment = verifier.assessment(100);
    assert_eq!(assessment["compatible"], true);
    assert_eq!(assessment["maxRpmAvailable"], false);
    assert_eq!(assessment["revRatioAvailable"], false);
    assert_eq!(assessment["channels"]["rev_ratio"]["status"], "unknown");
    assert!(assessment["diagnostics"].to_string().contains("No redline"));
}
#[test]
fn current_physics_maximum_enables_ratio_when_acr_omits_the_static_maximum() {
    let mut verifier = Verifier::new(&identity()).unwrap();
    let mut s = sample(10, 2);
    s.statics[412..416].copy_from_slice(&0i32.to_le_bytes());
    s.physics.resize(game_telemetry::PHYSICS_EXTENDED_SIZE, 0);
    s.physics[game_telemetry::PHYSICS_CURRENT_MAX_RPM_OFFSET..game_telemetry::PHYSICS_EXTENDED_SIZE]
        .copy_from_slice(&8500i32.to_le_bytes());
    verifier.observe(&s, 0).unwrap();
    s.physics[..4].copy_from_slice(&11u32.to_le_bytes());
    verifier.observe(&s, 100).unwrap();
    let assessment = verifier.assessment(100);
    assert_eq!(assessment["compatible"], true);
    assert_eq!(assessment["maxRpmAvailable"], true);
    assert_eq!(assessment["revRatioAvailable"], true);
    assert_eq!(assessment["channels"]["max_rpm"]["value"], 8500);
    assert_eq!(assessment["channels"]["rev_ratio"]["status"], "ready");
    assert_eq!(assessment["layoutRevision"], "acr-prefix-3");
    assert!(!assessment["diagnostics"].to_string().contains("Maximum RPM is unavailable"));
}
#[test]
fn frozen_pause_replay_off_and_stale_samples_do_not_validate_native_readiness() {
    for (status, label) in [(0, "waiting"), (1, "replay"), (3, "paused")] {
        let mut verifier = Verifier::new(&identity()).unwrap();
        verifier.observe(&sample(10, status), 0).unwrap();
        verifier.observe(&sample(11, status), 100).unwrap();
        let assessed = verifier.assessment(100);
        assert_eq!(assessed["status"], label);
        assert_eq!(assessed["compatible"], false);
    }
    let mut frozen = Verifier::new(&identity()).unwrap();
    frozen.observe(&sample(10, 2), 0).unwrap();
    frozen.observe(&sample(10, 2), 100).unwrap();
    assert_eq!(frozen.assessment(PROBE_MS)["status"], "stale");
    frozen.observe(&sample(11, 2), 200).unwrap();
    assert_eq!(frozen.assessment(200)["compatible"], true);
    assert_eq!(
        frozen.assessment(200 + FRESHNESS_MS + 1)["compatible"],
        false
    );
}
#[test]
fn pid_reuse_and_synthetic_identity_are_rejected_and_new_identity_needs_baseline() {
    let mut verifier = Verifier::new(&identity()).unwrap();
    let mut s = sample(10, 2);
    s.source.epoch += 1;
    assert!(verifier
        .observe(&s, 0)
        .unwrap_err()
        .contains("identity changed"));
    s = sample(10, 2);
    s.source.id = "synthetic".into();
    assert!(verifier.observe(&s, 0).is_err());
    let mut current = identity();
    current.epoch += 1;
    let mut relaunched = Verifier::new(&current).unwrap();
    let mut s = sample(12, 2);
    s.source.epoch = current.epoch;
    assert!(!relaunched.observe(&s, 0).unwrap().fresh);
    s.physics[..4].copy_from_slice(&13u32.to_le_bytes());
    assert!(relaunched.observe(&s, 100).unwrap().fresh);
}
#[test]
fn unsupported_layout_context_or_invalid_required_values_stay_unverified() {
    let mut verifier = Verifier::new(&identity()).unwrap();
    let mut malformed = sample(10, 2);
    malformed.physics.truncate(1);
    assert!(verifier.observe(&malformed, 0).is_err());
    let mut missing = sample(10, 2);
    write_text(&mut missing.statics, 30, 15, "");
    assert!(verifier
        .observe(&missing, 0)
        .unwrap_err()
        .contains("version"));
    let mut missing = sample(10, 2);
    write_text(&mut missing.statics, 68, 33, "");
    assert_eq!(
        failure_status(&verifier.observe(&missing, 0).unwrap_err()),
        "waiting"
    );
    let mut s = sample(10, 2);
    s.physics[20..24].copy_from_slice(&(-1i32).to_le_bytes());
    verifier.observe(&s, 0).unwrap();
    s.physics[..4].copy_from_slice(&11u32.to_le_bytes());
    verifier.observe(&s, 100).unwrap();
    let assessed = verifier.assessment(100);
    assert_eq!(assessed["compatible"], false);
    assert_eq!(assessed["status"], "unsupported-layout");
}
#[test]
fn context_change_and_packet_reset_require_new_live_baselines() {
    let mut verifier = Verifier::new(&identity()).unwrap();
    verifier.observe(&sample(10, 2), 0).unwrap();
    assert!(verifier.observe(&sample(11, 2), 100).unwrap().fresh);
    let mut next = sample(12, 2);
    write_text(&mut next.statics, 134, 33, "next_stage");
    assert!(!verifier.observe(&next, 200).unwrap().fresh);
    next.physics[..4].copy_from_slice(&13u32.to_le_bytes());
    assert!(verifier.observe(&next, 300).unwrap().fresh);
    next.physics[..4].copy_from_slice(&0u32.to_le_bytes());
    assert!(!verifier.observe(&next, 400).unwrap().fresh);
    next.physics[..4].copy_from_slice(&1u32.to_le_bytes());
    assert!(verifier.observe(&next, 500).unwrap().fresh);
}
#[test]
fn unsigned_packet_wrap_advances_without_resetting_valid_context() {
    let mut verifier = Verifier::new(&identity()).unwrap();
    verifier.observe(&sample(u32::MAX, 2), 0).unwrap();
    assert!(verifier.observe(&sample(0, 2), 100).unwrap().fresh);
}
#[test]
fn native_errors_have_actionable_waiting_access_exit_conflict_or_layout_statuses() {
    for (error, status) in [
        (
            "ACR telemetry page is not available: file not found (os error 2)",
            "waiting",
        ),
        (
            "Cannot open process: Access is denied (os error 5)",
            "access-denied",
        ),
        ("Selected ACR process exited", "process-exited"),
        (
            "Invalid physics telemetry page length",
            "unsupported-layout",
        ),
        (
            "Another Assetto process is running (acs.exe)",
            "conflicting-producer",
        ),
        (
            "ACR telemetry changed during all bounded snapshot attempts.",
            "waiting",
        ),
    ] {
        assert_eq!(failure_status(error), status);
    }
    let value = result(
        &json!({"fingerprint":"sha256:authored"}),
        "waiting",
        vec!["Start a session".into()],
    );
    assert_eq!(value["nativeVerified"], false);
    assert_eq!(value["gameFilesChanged"], false);
}
struct FakePlatform;
impl Platform for FakePlatform {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        Err("fixture".into())
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(None)
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        Ok(false)
    }
}
#[test]
fn cancellation_does_not_construct_a_native_source_or_change_game_files() {
    let scratch = Scratch::new();
    let path = scratch.executable();
    let bytes = fs::read(&path).unwrap();
    let game = json!({"executable":path});
    assert!(probe(&game, &FakePlatform, || true, |_, _| {})
        .unwrap_err()
        .contains("cancelled"));
    assert_eq!(fs::read(path).unwrap(), bytes);
}
#[cfg(not(windows))]
#[test]
fn non_windows_and_synthetic_fixtures_cannot_report_native_verification() {
    let scratch = Scratch::new();
    let path = scratch.executable();
    let game = json!({"executable":path});
    let status = probe(&game, &FakePlatform, || false, |_, _| {}).unwrap();
    assert_eq!(status["status"], "unsupported-platform");
    assert_eq!(status["nativeVerified"], false);
    assert!(status["fingerprint"]["fingerprint"].as_str().is_some());
    assert!(competing_processes(7).is_err());
    assert!(ensure_single_producer(7).is_err());
}

#[test]
fn root_bootstrap_fingerprints_the_actual_existing_unreal_runtime() {
    let scratch = Scratch::new();
    let root = scratch.executable();
    let runtime = scratch.0.join("acr/Binaries/Win64/acr.exe");
    fs::create_dir_all(runtime.parent().unwrap()).unwrap();
    fs::write(&runtime, b"authored unreal runtime fixture").unwrap();
    let selected = fingerprint(&root).unwrap();
    let actual = fingerprint(&runtime).unwrap();
    assert_eq!(selected["fingerprint"], actual["fingerprint"]);
    assert_eq!(selected["executable"], actual["executable"]);
}

use std::{
    cell::Cell,
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
struct ProbePlatform {
    selected: Option<ProcessIdentity>,
    lifetime: Mutex<VecDeque<Result<bool, String>>>,
}
impl ProbePlatform {
    fn running(process: ProcessIdentity) -> Self {
        Self {
            selected: Some(process),
            lifetime: Mutex::new(VecDeque::new()),
        }
    }
}
impl Platform for ProbePlatform {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("Read-only probe must not launch a game")
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(self.selected.clone())
    }
    fn alive(&self, process: &ProcessIdentity) -> Result<bool, String> {
        assert_eq!(self.selected.as_ref().unwrap().epoch, process.epoch);
        self.lifetime
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(true))
    }
}
/// Authored source exists only in this test module; it cannot enter the Windows boundary.
struct AuthoredAcquisition {
    polls: VecDeque<Result<Option<RawSample>, String>>,
    closed: Arc<AtomicBool>,
    on_poll: Option<Box<dyn FnMut() + Send>>,
}
impl Source for AuthoredAcquisition {
    fn poll(&mut self) -> Result<Option<RawSample>, String> {
        assert!(!self.closed.load(Ordering::Acquire));
        if let Some(action) = self.on_poll.as_mut() {
            action();
        }
        self.polls.pop_front().unwrap_or(Ok(None))
    }
    fn close(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}
struct ProbeHarness {
    scratch: Scratch,
    game: Value,
    fingerprint: Value,
    process: ProcessIdentity,
}
impl ProbeHarness {
    fn new() -> Self {
        let scratch = Scratch::new();
        let executable = scratch.executable();
        let fingerprint = fingerprint(&executable).unwrap();
        let process = ProcessIdentity {
            path: fingerprint["executable"].as_str().unwrap().into(),
            ..identity()
        };
        Self {
            scratch,
            game: json!({"executable":executable}),
            fingerprint,
            process,
        }
    }
    fn run(
        &self,
        platform: &dyn Platform,
        polls: Vec<Result<Option<RawSample>, String>>,
        image: Option<String>,
        competitors: impl Fn(u32) -> Result<Vec<Value>, String>,
        check: impl Fn() -> bool,
        on_poll: Option<Box<dyn FnMut() + Send>>,
    ) -> (Result<Value, String>, Arc<AtomicBool>) {
        let closed = Arc::new(AtomicBool::new(false));
        let source = AuthoredAcquisition {
            polls: polls.into(),
            closed: closed.clone(),
            on_poll,
        };
        let now = Cell::new(0);
        let waits = Cell::new(0);
        let result = probe_acquired(
            &self.game,
            platform,
            &self.fingerprint,
            check,
            |_, percent| assert!((15..=90).contains(&percent)),
            |_| {
                Ok(AttachedSource {
                    source: Box::new(source),
                    image,
                })
            },
            competitors,
            || {
                let current = now.get();
                now.set(current + 500);
                current
            },
            || {
                waits.set(waits.get() + 1);
                assert!(waits.get() <= 5, "Probe exceeded its deadline");
            },
        );
        (result, closed)
    }
    fn frames(&self, frames: Vec<RawSample>) -> (Value, Arc<AtomicBool>) {
        let (proof, closed) = self.run(
            &ProbePlatform::running(self.process.clone()),
            frames.into_iter().map(|sample| Ok(Some(sample))).collect(),
            Some(self.process.path.clone()),
            |_| Ok(vec![]),
            || false,
            None,
        );
        (proof.unwrap(), closed)
    }
}
#[test]
fn closed_or_wrong_selected_process_never_opens_an_acquisition() {
    let harness = ProbeHarness::new();
    for (selected, expected) in [
        (None, "waiting"),
        (
            Some(ProcessIdentity {
                path: harness
                    .scratch
                    .0
                    .join("other/acr.exe")
                    .display()
                    .to_string(),
                ..harness.process.clone()
            }),
            "process-exited",
        ),
    ] {
        let platform = ProbePlatform {
            selected,
            lifetime: Mutex::new(VecDeque::new()),
        };
        let proof = probe_acquired(
            &harness.game,
            &platform,
            &harness.fingerprint,
            || false,
            |_, _| {},
            |_| panic!("Unmatched/closed process must not acquire mappings"),
            |_| panic!("Unmatched/closed process must not query mapping producers"),
            || 0,
            || {},
        )
        .unwrap();
        assert_eq!(proof["status"], expected);
        assert_eq!(proof["nativeVerified"], false);
        assert_eq!(proof["fingerprint"], harness.fingerprint);
    }
}
#[test]
fn producer_conflicts_are_checked_before_attach_during_poll_and_before_qualification() {
    let harness = ProbeHarness::new();
    for conflict_at in [1, 2, 4] {
        let queries = Cell::new(0);
        let (result, closed) = harness.run(
            &ProbePlatform::running(harness.process.clone()),
            vec![Ok(Some(sample(10, 2))), Ok(Some(sample(11, 2)))],
            Some(harness.process.path.clone()),
            |_| {
                queries.set(queries.get() + 1);
                Ok(if queries.get() == conflict_at {
                    vec![json!({"pid":901,"image":"acs.exe"})]
                } else {
                    vec![]
                })
            },
            || false,
            None,
        );
        let proof = result.unwrap();
        assert_eq!(proof["status"], "conflicting-producer");
        assert!(proof["diagnostics"].to_string().contains("PID 901"));
        assert_eq!(proof["nativeVerified"], false);
        assert_eq!(closed.load(Ordering::Acquire), conflict_at != 1);
    }
}
#[test]
fn attachment_access_failure_and_image_handoff_mismatch_have_distinct_diagnostics() {
    let harness = ProbeHarness::new();
    let platform = ProbePlatform::running(harness.process.clone());
    let denied = probe_acquired(
        &harness.game,
        &platform,
        &harness.fingerprint,
        || false,
        |_, _| {},
        |_| Err("Cannot open process: Access is denied (os error 5)".into()),
        |_| Ok(vec![]),
        || 0,
        || {},
    )
    .unwrap();
    assert_eq!(denied["status"], "access-denied");
    assert_eq!(denied["nativeVerified"], false);
    for image in [None, Some("C:/Other/acr.exe".into())] {
        let (result, closed) =
            harness.run(&platform, vec![], image, |_| Ok(vec![]), || false, None);
        assert_eq!(result.unwrap()["status"], "process-exited");
        assert!(closed.load(Ordering::Acquire));
    }
}
#[test]
fn cancelled_exited_or_failed_lifetime_queries_close_the_acquisition() {
    let harness = ProbeHarness::new();
    let cancelled = Cell::new(0);
    let (result, closed) = harness.run(
        &ProbePlatform::running(harness.process.clone()),
        vec![Ok(Some(sample(10, 2)))],
        Some(harness.process.path.clone()),
        |_| Ok(vec![]),
        || {
            cancelled.set(cancelled.get() + 1);
            cancelled.get() == 2
        },
        None,
    );
    assert!(result.unwrap_err().contains("cancelled"));
    assert!(closed.load(Ordering::Acquire));
    for lifetime in [
        vec![Ok(false)],
        vec![Ok(true), Ok(true), Ok(false)],
        vec![Err("Cannot query process lifetime".into())],
    ] {
        let platform = ProbePlatform::running(harness.process.clone());
        *platform.lifetime.lock().unwrap() = lifetime.into();
        let (result, closed) = harness.run(
            &platform,
            vec![Ok(Some(sample(10, 2))), Ok(Some(sample(11, 2)))],
            Some(harness.process.path.clone()),
            |_| Ok(vec![]),
            || false,
            None,
        );
        match result {
            Ok(proof) => {
                assert_eq!(proof["status"], "process-exited");
                assert_eq!(proof["nativeVerified"], false);
            }
            Err(error) => assert!(error.contains("lifetime")),
        }
        assert!(closed.load(Ordering::Acquire));
    }
}
#[test]
fn missing_mappings_wait_to_deadline_but_access_denial_and_invalid_samples_fail_immediately() {
    let harness = ProbeHarness::new();
    let mut invalid = sample(10, 2);
    invalid.physics.truncate(1);
    for (polls, status) in [
        (
            vec![Err(
                "ACR telemetry page is not available (os error 2)".into()
            )],
            "waiting",
        ),
        (
            vec![Err("Access is denied (os error 5)".into())],
            "access-denied",
        ),
        (vec![Ok(Some(invalid))], "unsupported-layout"),
    ] {
        let (result, closed) = harness.run(
            &ProbePlatform::running(harness.process.clone()),
            polls,
            Some(harness.process.path.clone()),
            |_| Ok(vec![]),
            || false,
            None,
        );
        let proof = result.unwrap();
        assert_eq!(proof["status"], status);
        assert_eq!(proof["nativeVerified"], false);
        assert!(closed.load(Ordering::Acquire));
    }
}
#[test]
fn authored_compatible_frames_exercise_full_qualification_but_never_verify_native() {
    let harness = ProbeHarness::new();
    let (proof, closed) = harness.frames(vec![sample(10, 2), sample(11, 2)]);
    assert_eq!(proof["compatible"], true);
    assert_eq!(proof["status"], "receiving");
    assert_eq!(proof["nativeVerified"], false);
    assert_eq!(proof["gameFilesChanged"], false);
    for channel in ["rpm", "gear", "speed_kmh"] {
        assert_eq!(proof["channels"][channel]["status"], "ready");
    }
    assert!(closed.load(Ordering::Acquire));
}
#[test]
fn probe_orchestration_preserves_pause_replay_menu_frozen_and_invalid_value_diagnostics() {
    let harness = ProbeHarness::new();
    for (status, expected) in [(0, "waiting"), (1, "replay"), (3, "paused")] {
        let (proof, closed) =
            harness.frames((10..16).map(|packet| sample(packet, status)).collect());
        assert_eq!(proof["status"], expected);
        assert_eq!(proof["nativeVerified"], false);
        assert!(closed.load(Ordering::Acquire));
    }
    let (frozen, _) = harness.frames(vec![sample(10, 2), sample(10, 2)]);
    assert_eq!(frozen["status"], "stale");
    assert_eq!(frozen["nativeVerified"], false);
    let invalid_frames = (10..16)
        .map(|packet| {
            let mut s = sample(packet, 2);
            s.physics[20..24].copy_from_slice(&(-1i32).to_le_bytes());
            s
        })
        .collect();
    let (invalid, _) = harness.frames(invalid_frames);
    assert_eq!(invalid["status"], "unsupported-layout");
    assert_eq!(invalid["nativeVerified"], false);
}
#[test]
fn executable_update_and_source_epoch_reuse_cannot_qualify_previous_evidence() {
    let harness = ProbeHarness::new();
    let path = PathBuf::from(harness.game["executable"].as_str().unwrap());
    let (result, closed) = harness.run(
        &ProbePlatform::running(harness.process.clone()),
        vec![Ok(Some(sample(10, 2))), Ok(Some(sample(11, 2)))],
        Some(harness.process.path.clone()),
        |_| Ok(vec![]),
        || false,
        Some(Box::new(move || {
            fs::write(&path, b"updated authored executable").unwrap()
        })),
    );
    let proof = result.unwrap();
    assert_eq!(proof["status"], "executable-changed");
    assert_ne!(
        proof["fingerprint"]["fingerprint"],
        harness.fingerprint["fingerprint"]
    );
    assert_eq!(proof["nativeVerified"], false);
    assert!(closed.load(Ordering::Acquire));
    let harness = ProbeHarness::new();
    let mut reused = sample(10, 2);
    reused.source.epoch += 1;
    let (proof, closed) = harness.frames(vec![reused]);
    assert_eq!(proof["status"], "process-exited");
    assert_eq!(proof["nativeVerified"], false);
    assert!(closed.load(Ordering::Acquire));
}
#[test]
fn producer_query_failure_closes_attached_source_instead_of_using_unqualified_packets() {
    let harness = ProbeHarness::new();
    let queries = Cell::new(0);
    let (result, closed) = harness.run(
        &ProbePlatform::running(harness.process.clone()),
        vec![Ok(Some(sample(10, 2)))],
        Some(harness.process.path.clone()),
        |_| {
            queries.set(queries.get() + 1);
            if queries.get() == 2 {
                Err("Process query denied".into())
            } else {
                Ok(vec![])
            }
        },
        || false,
        None,
    );
    assert_eq!(result.unwrap_err(), "Process query denied");
    assert!(closed.load(Ordering::Acquire));
}

#[test]
fn live_observed_sparse_status_verifies_required_channels_and_reports_state_limit() {
    let mut verifier = Verifier::new(&identity()).unwrap();
    for packet in [10, 11] {
        let mut s = sample(packet, 0);
        s.statics[..60].fill(0);
        s.statics[412..416].fill(0);
        verifier.observe(&s, packet as u64).unwrap();
    }
    let result = verifier.assessment(12);
    assert_eq!(result["compatible"], true);
    assert_eq!(result["status"], "receiving");
    assert_eq!(result["maxRpmAvailable"], false);
    assert!(result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d.as_str().unwrap().contains("pause/menu/replay")));
}

#[cfg(windows)]
#[test]
#[ignore = "requires OSG_ACR_LIVE_EXE and a running ACR driving session"]
fn running_game_passes_native_setup_probe() {
    let executable = std::env::var("OSG_ACR_LIVE_EXE").expect("set live game executable");
    let game = json!({"id":"assetto-corsa-rally","executable":executable});
    let proof = probe(
        &game,
        &crate::game_discovery::NativePlatform,
        || false,
        |_, _| {},
    )
    .unwrap();
    println!("LIVE ACR SETUP {proof}");
    assert_eq!(proof["nativeVerified"], true);
    assert_eq!(proof["compatible"], true);
}

#[test]
fn producer_conflicts_are_limited_to_the_selected_mapping_namespace() {
    for game in [Game::Corsa, Game::Competizione, Game::Rally] {
        for image in ["acs.exe", "acs_x86.exe", "AC2-Win64-Shipping.exe", "acr.exe"] {
            assert!(shares_mapping(game, image));
            assert!(!shares_mapping(Game::Evo, image));
        }
        assert!(!shares_mapping(game, "AssettoCorsaEVO.exe"));
    }
    assert!(shares_mapping(Game::Evo, "C:/Games/AssettoCorsaEVO.exe"));
    for image in ["AssettoCorsa.exe", "AC2.exe", "Cockpit.exe"] {
        for game in [Game::Corsa, Game::Competizione, Game::Evo, Game::Rally] { assert!(!shares_mapping(game, image)); }
    }
}
#[test]
fn passive_fingerprint_cache_reuses_hashes_and_invalidates_every_metadata_identity_change() {
    let scratch = Scratch::new();
    let path = scratch.executable();
    let reads = std::cell::Cell::new(0);
    let mut hash = |path: &Path, game| { reads.set(reads.get() + 1); fingerprint_for_game(path, game) };
    let mut cache = FingerprintCache::default();
    let first = cache.inspect_with(&path, Game::Rally, false, &mut hash).unwrap();
    for _ in 0..10 { assert_eq!(cache.inspect_with(&path, Game::Rally, false, &mut hash).unwrap(), first); }
    assert_eq!(reads.get(), 1);
    let copy = scratch.0.join("copy/acr.exe");
    fs::create_dir(copy.parent().unwrap()).unwrap();
    fs::copy(&path, &copy).unwrap();
    cache.inspect_with(&copy, Game::Rally, false, &mut hash).unwrap();
    assert_eq!(reads.get(), 2);
    fs::write(&copy, b"Updated game build").unwrap();
    let changed = cache.inspect_with(&copy, Game::Rally, false, &mut hash).unwrap();
    assert_eq!(reads.get(), 3);
    assert_ne!(changed["fingerprint"], first["fingerprint"]);
    let modified = fs::metadata(&copy).unwrap().modified().unwrap() + Duration::from_secs(2);
    fs::File::options().write(true).open(&copy).unwrap().set_modified(modified).unwrap();
    cache.inspect_with(&copy, Game::Rally, false, &mut hash).unwrap();
    assert_eq!(reads.get(), 4);
    cache.inspect_with(&copy, Game::Rally, true, &mut hash).unwrap();
    assert_eq!(reads.get(), 5);
    fs::remove_file(&copy).unwrap();
    assert!(cache.inspect_with(&copy, Game::Rally, false, &mut hash).is_err());
    assert!(cache.entries.is_empty());
}
#[test]
fn explicit_verification_bypasses_unchanged_metadata_and_cache_stays_bounded_by_game() {
    let scratch = Scratch::new();
    let mut cache = FingerprintCache::default();
    for (game, name) in [(Game::Corsa,"acs.exe"),(Game::Competizione,"AC2-Win64-Shipping.exe"),(Game::Evo,"AssettoCorsaEVO.exe"),(Game::Rally,"acr.exe")] {
        let path = scratch.0.join(name);
        fs::write(&path, b"original").unwrap();
        let first = cache.inspect(&path, game, false).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, b"modified").unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
        // New setup/check and fresh preparation always hash, even if an external
        // tool preserved the previous executable's size and modification time.
        assert_ne!(cache.inspect(&path, game, true).unwrap()["fingerprint"], first["fingerprint"]);
    }
    assert_eq!(cache.entries.len(), 4);
}
#[test]
fn runtime_resolution_never_substitutes_an_unrelated_rally_executable_for_corsa_or_evo() {
    let scratch = Scratch::new();
    let nested = scratch.0.join("acr/Binaries/Win64/acr.exe");
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::write(&nested, b"unrelated").unwrap();
    for (game, name) in [(Game::Corsa,"acs.exe"),(Game::Evo,"AssettoCorsaEVO.exe")] {
        let selected = scratch.0.join(name); fs::write(&selected,b"selected").unwrap();
        assert_eq!(fingerprint_for_game(&selected,game).unwrap()["executable"],json!(fs::canonicalize(selected).unwrap()));
    }
}
