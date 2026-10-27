use super::*;
use crate::{game_discovery::{Platform, ProcessIdentity}, game_library::{Candidate, Scan}};
use std::{collections::BTreeMap, sync::mpsc};

struct EmptyPlatform;
impl Platform for EmptyPlatform {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> { vec![] }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> { Ok(()) }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> { Err("Not used in discovery tests.".into()) }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> { Ok(None) }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> { Ok(false) }
}

fn runtime() -> (GamesRuntime, tempfile::TempDir) {
    let directory = crate::test_directory();
    let runtime = GamesRuntime::test_runtime(
        Arc::new(|_| json!({})),
        crate::games::Paths::in_directory(directory.path()),
        Arc::new(EmptyPlatform),
    );
    (runtime, directory)
}

fn candidate(root: &Path, client: &str, app_id: &str) -> Candidate {
    std::fs::create_dir_all(root).unwrap();
    let executable = root.join("Game.exe");
    std::fs::write(&executable, b"fixture").unwrap();
    Candidate {
        id: format!("{client}:{app_id}"),
        client: client.into(),
        name: "Fixture title".into(),
        install_path: root.to_string_lossy().into_owned(),
        executable_candidates: vec![executable.to_string_lossy().into_owned()],
        preferred_executable: None,
        game_id: None,
        kind: "game".into(),
        app_id: app_id.into(),
        launcher: None,
        artwork: None,
    }
}

fn wait_for(runtime: &GamesRuntime, expected: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let discovery = runtime.status()["discovery"].clone();
        if discovery["status"] == expected { return discovery; }
        assert!(Instant::now() < deadline, "discovery status did not become {expected}: {discovery}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn platform_metadata_is_read_without_holding_progress_or_importing_before_apply() {
    let (runtime,directory)=runtime();
    let steam=candidate(&directory.path().join("steam"),"steam","42");
    let (entered,waiting)=mpsc::channel();let (release,resume)=mpsc::channel();
    start_with_metadata(&runtime,&json!({"command":"game_discovery_start"}),vec![("steam","Steam")],
        move|_,_,_|Scan{candidates:vec![steam.clone()],diagnostics:vec![]},
        move|_,_,_,cancelled|{entered.send(()).unwrap();resume.recv().unwrap();assert!(!cancelled());BTreeMap::from([("steam:42".into(),super::super::tagging::SourceMetadata{platform_genres:vec!["Racing".into()],..Default::default()})])}).unwrap();
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    let progress=runtime.status()["discovery"].clone();
    assert_eq!(progress["status"],"scanning");assert_eq!(progress["libraries"][0]["status"],"preparing");
    assert_eq!(progress["entries"].as_array().unwrap().len(),1);
    assert!(runtime.inner.settings.lock().unwrap().saved.game_tags.is_empty());
    release.send(()).unwrap();let ready=wait_for(&runtime,"ready");
    assert_eq!(ready["entries"][0]["tagIds"],json!(["racing"]));
    assert!(runtime.inner.settings.lock().unwrap().saved.game_tags.is_empty());runtime.shutdown();
}

#[test]
fn closing_during_metadata_prevents_completion() {
    let (runtime,directory)=runtime();let steam=candidate(&directory.path().join("steam"),"steam","42");
    let (entered,waiting)=mpsc::channel();let (release,resume)=mpsc::channel();
    start_with_metadata(&runtime,&json!({"command":"game_discovery_start"}),vec![("steam","Steam")],
        move|_,_,_|Scan{candidates:vec![steam.clone()],diagnostics:vec![]},
        move|_,_,_,_|{entered.send(()).unwrap();resume.recv().unwrap();BTreeMap::new()}).unwrap();
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();runtime.inner.closed.store(true,Ordering::Release);
    release.send(()).unwrap();runtime.shutdown();
    assert_eq!(runtime.inner.discovery.lock().unwrap().status,"scanning");
}

#[test]
fn injected_scans_publish_rows_retain_failures_and_reject_a_parallel_scan() {
    let (runtime, directory) = runtime();
    let mut steam = candidate(&directory.path().join("steam"), "steam", "123");
    let artwork = directory.path().join("header.jpg");
    image::RgbImage::from_pixel(920, 430, image::Rgb([170, 80, 40])).save(&artwork).unwrap();
    steam.artwork = Some(artwork);
    let installation = steam.install_path.clone();
    let disk = candidate(&directory.path().join("disk"), "disk", "fixture");
    let (entered, waiting) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let started = json!({"command":"game_discovery_start","paths":[]});
    start_with(
        &runtime,
        &started,
        vec![("steam", "Steam"), ("disk", "Local paths")],
        move |client, _| {
            if client == "steam" {
                entered.send(()).unwrap();
                resume.recv().unwrap();
                Scan {
                    candidates: vec![steam.clone()],
                    diagnostics: vec!["Steam manifest could not be read.".into()],
                }
            } else {
                Scan { candidates: vec![disk.clone()], diagnostics: vec![] }
            }
        },
    ).unwrap();
    waiting.recv_timeout(Duration::from_secs(2)).expect("scan closure should start");

    let parallel = start_with(
        &runtime,
        &started,
        vec![("disk", "Local paths")],
        |_, _| panic!("the running guard must reject before invoking a scan"),
    );
    release.send(()).unwrap();
    assert!(parallel.unwrap_err().contains("already running"));

    let ready = wait_for(&runtime, "ready");
    assert_eq!(ready["entries"].as_array().unwrap().len(), 2);
    assert_eq!(ready["libraries"][0]["status"], "failed");
    assert_eq!(ready["libraries"][1]["status"], "complete");
    assert!(ready["entries"].as_array().unwrap().iter().any(|entry| entry["image"].is_string()));
    assert!(crate::game_artwork::installation_preview(runtime.inner.installations.root(), &installation).is_some());
    assert!(runtime.inner.settings.lock().unwrap().saved.games.is_empty());
    assert!(runtime.status()["catalogue"].as_array().unwrap().is_empty());
    runtime.shutdown();
}

#[test]
fn panicked_scan_fails_cleanly_and_a_new_scan_can_finish() {
    let (runtime, directory) = runtime();
    let request = json!({"command":"game_discovery_start","paths":[]});
    start_with(
        &runtime,
        &request,
        vec![("disk", "Local paths")],
        |_, _| panic!("injected scan failure"),
    ).unwrap();
    let failed = wait_for(&runtime, "failed");
    assert_eq!(failed["diagnostics"][0], "Game discovery failed. Rescan to try again.");

    let discovered = candidate(&directory.path().join("retry"), "disk", "retry");
    start_with(
        &runtime,
        &request,
        vec![("disk", "Local paths")],
        move |_, _| Scan { candidates: vec![discovered.clone()], diagnostics: vec![] },
    ).unwrap();
    let ready = wait_for(&runtime, "ready");
    assert_eq!(ready["entries"].as_array().unwrap().len(), 1);
    assert!(runtime.inner.settings.lock().unwrap().saved.games.is_empty());
    runtime.shutdown();
}

#[test]
fn public_start_rejects_invalid_requests_without_launching_a_worker() {
    let (runtime, _directory) = runtime();
    let error = start(&runtime, &json!({"command":"game_discovery_start","unexpected":true}))
        .unwrap_err();
    assert!(error.contains("Unknown discovery start field"));
    assert_eq!(runtime.status()["discovery"]["status"], "idle");
    assert!(runtime.discovery_worker.lock().unwrap().is_none());
    runtime.shutdown();
}

#[test]
fn only_added_paths_enable_standalone_scanning_after_platforms_with_occupied_roots() {
    let (runtime,directory)=runtime();let parent=directory.path().join("Program Files");
    let steam=candidate(&parent.join("Steam/steamapps/common/SteamVR"),"steam","42");
    candidate(&parent.join("Steam"),"disk","launcher");
    candidate(&parent.join("Standalone"),"disk","standalone");
    let steam_root=steam.install_path.clone();let added=parent.clone();
    start_with_metadata(&runtime,&json!({"command":"game_discovery_start","paths":[parent]}),vec![("steam","Steam")],
        move|client,roots,occupied|{
            assert_eq!(roots,&[added.clone()]);
            if client=="steam" {Scan{candidates:vec![steam.clone()],diagnostics:vec![]}}
            else {assert_eq!(client,"disk");assert!(occupied.iter().any(|path|path==Path::new(&steam_root)));
                crate::game_library::scan_user_paths(roots,occupied)}
        },|_,_,_,_|BTreeMap::new()).unwrap();
    let ready=wait_for(&runtime,"ready");
    assert_eq!(ready["libraries"].as_array().unwrap().len(),2);
    assert_eq!(ready["libraries"][1]["name"],"Added paths");
    let entries=ready["entries"].as_array().unwrap();assert_eq!(entries.len(),2);
    assert_eq!(entries.iter().filter(|entry|entry["library"]=="disk").count(),1);
    assert!(entries.iter().any(|entry|entry["installPath"].as_str().unwrap().ends_with("Standalone")));
    runtime.shutdown();
}

#[test]
fn saved_game_roots_are_reserved_but_never_automatically_scanned() {
    let (runtime,directory)=runtime();let found=candidate(&directory.path().join("used"),"steam","42");
    let saved=crate::game_installations::Settings::default();
    let review=Review{scan_id:"fixture".into(),status:"ready".into(),revision:saved.revision,
        entries:super::super::test_reconcile(&saved,vec![found]),..Default::default()};
    let saved=super::super::apply(&saved,&review,&json!({"scanId":"fixture","actions":[]})).unwrap();
    runtime.inner.settings.lock().unwrap().saved=saved;
    start_with_metadata(&runtime,&json!({"command":"game_discovery_start"}),vec![],
        |_,_,_|panic!("No added paths or platform clients means no scan"),|_,_,_,_|BTreeMap::new()).unwrap();
    let ready=wait_for(&runtime,"ready");assert!(ready["libraries"].as_array().unwrap().is_empty());runtime.shutdown();
}

#[test]
fn previously_added_containers_are_rescanned_without_new_path_arguments() {
    let (runtime,directory)=runtime();let path=directory.path().to_path_buf();
    runtime.inner.settings.lock().unwrap().saved.scan_roots.push(path.to_string_lossy().into_owned());
    start_with(&runtime,&json!({"command":"game_discovery_start"}),vec![],move|client,roots|{
        assert_eq!(client,"disk");assert_eq!(roots,&[path.clone()]);Scan::default()
    }).unwrap();
    let ready=wait_for(&runtime,"ready");assert_eq!(ready["libraries"][0]["name"],"Added paths");runtime.shutdown();
}

#[test]
fn added_paths_require_containers_and_cannot_be_executable_files() {
    let (runtime,directory)=runtime();let path=directory.path().join("Game.exe");std::fs::write(&path,b"fixture").unwrap();
    let error=start(&runtime,&json!({"command":"game_discovery_start","paths":[path]})).unwrap_err();
    assert!(error.contains("folders"));assert_eq!(runtime.status()["discovery"]["status"],"idle");runtime.shutdown();
}
