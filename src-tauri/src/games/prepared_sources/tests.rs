use super::*;
use crate::game_discovery::ProcessIdentity;
use std::{collections::BTreeMap, fs, sync::mpsc};

struct PlatformFixture;
impl Platform for PlatformFixture {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> { vec![] }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> { Ok(()) }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> { panic!("No game launch") }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> { Ok(None) }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> { Ok(false) }
}
fn fixture() -> (tempfile::TempDir, Arc<GamesRuntime>, String) {
    let root = crate::test_directory();
    let runtime = Arc::new(GamesRuntime::test_runtime(Arc::new(|_| panic!("No outputs")),
        Paths::in_directory(root.path()), Arc::new(PlatformFixture)));
    let executable = root.path().join("fixture.exe");fs::write(&executable, b"metadata fixture").unwrap();
    let image = root.path().join("image.png");
    image::RgbImage::from_pixel(80, 50, image::Rgb([80u8, 20, 40])).save(&image).unwrap();
    runtime.metadata_request(&json!({"command":"game_add","name":"Prepared image","installPath":root.path(),
        "executable":executable,"imagePath":image})).unwrap();
    let id = runtime.status()["addedGameId"].as_str().unwrap().to_owned();
    (root, runtime, id)
}
fn image(runtime: &GamesRuntime, id: &str) -> Option<String> {
    runtime.status()["catalogue"].as_array().unwrap().iter().find(|game| game["id"] == id)
        .and_then(|game| game["background"]["image"].as_str()).map(str::to_owned)
}
fn settled(runtime: &GamesRuntime, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if predicate() { return; }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("Native artwork source did not settle: {:?}",runtime.inner.prepared_sources.error());
}
fn asset(runtime: &GamesRuntime, id: &str) -> PathBuf {
    let settings = runtime.inner.settings.lock().unwrap();
    let reference = &settings.saved.backgrounds[id].image;
    runtime.inner.installations.root().join("game-artwork").join(reference.strip_prefix("artwork:").unwrap())
}

#[test]
fn blocked_source_preparation_does_not_block_status_or_metadata_admission() {
    let (_root, runtime, id) = fixture();let original = image(&runtime,&id).unwrap();
    let (entered,observed) = mpsc::channel();let (release,blocked) = mpsc::channel();
    *runtime.inner.prepared_sources.pause.lock().unwrap() = Some((entered,blocked));
    runtime.inner.prepared_sources.request(true,None);
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    let reader = runtime.clone();let reader_id = id.clone();let (send,receive) = mpsc::channel();
    let task = thread::spawn(move || {
        let admission = reader.admission.try_lock().is_ok();
        send.send((image(&reader,&reader_id),admission)).unwrap();
    });
    let captured = receive.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();task.join().unwrap();
    assert_eq!(captured.unwrap(),(Some(original),true));
    runtime.inner.prepared_sources.refresh(false);
}

#[test]
fn native_file_changes_withdraw_and_restore_prepared_images_without_status_collection() {
    let (_root,runtime,id) = fixture();let original = image(&runtime,&id).unwrap();
    let path = asset(&runtime,&id);let bytes = fs::read(&path).unwrap();
    // An old authoritative path cache must not hide same-reference tampering.
    let mut old = runtime.inner.settings.lock().unwrap().catalogue.clone();
    crate::game_artwork::decorate(runtime.inner.installations.root(),&mut old);
    fs::write(&path,b"changed content").unwrap();
    settled(&runtime,||image(&runtime,&id).is_none());
    fs::remove_file(&path).unwrap();fs::write(&path,&bytes).unwrap();
    settled(&runtime,||image(&runtime,&id).as_ref() == Some(&original));
    fs::remove_file(&path).unwrap();
    settled(&runtime,||image(&runtime,&id).is_none());
}

#[test]
fn a_new_metadata_generation_fences_a_delayed_artwork_preparation() {
    let (root,runtime,id) = fixture();let original = image(&runtime,&id).unwrap();
    let new_image = root.path().join("replacement.png");
    image::RgbImage::from_pixel(80,50,image::Rgb([10u8,180,50])).save(&new_image).unwrap();
    let prepared = runtime.prepare_metadata(&json!({"command":"game_background_save","gameId":id,"path":new_image})).unwrap();
    let (entered,observed) = mpsc::channel();let (release,blocked) = mpsc::channel();
    *runtime.inner.prepared_sources.pause.lock().unwrap() = Some((entered,blocked));
    runtime.inner.prepared_sources.request(true,None);observed.recv_timeout(Duration::from_secs(2)).unwrap();
    runtime.commit_metadata(prepared).unwrap();
    assert_ne!(image(&runtime,&id).as_ref(),Some(&original));
    release.send(()).unwrap();runtime.inner.prepared_sources.refresh(false);
    assert!(image(&runtime,&id).is_some_and(|next|next != original));
    let state = runtime.inner.prepared_sources.state.lock().unwrap();
    assert_eq!(state.handled,state.requested);
}

#[test]
fn durable_metadata_commit_does_not_hold_the_passive_reader_mutex() {
    let (_root,runtime,id) = fixture();
    let prepared = runtime.prepare_metadata(&json!({"command":"game_save","gameId":id,"name":"Updated"})).unwrap();
    let (entered,observed) = mpsc::channel();let (release,blocked) = mpsc::channel();
    *runtime.inner.metadata_commit_pause.lock().unwrap() = Some((entered,blocked));
    let writer = runtime.clone();let task = thread::spawn(move || writer.commit_metadata(prepared));
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    let reader = runtime.clone();let (send,receive) = mpsc::channel();
    let read_task = thread::spawn(move || send.send(reader.committed_metadata_for_test()).unwrap());
    let captured = receive.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();task.join().unwrap().unwrap();read_task.join().unwrap();
    assert_eq!(captured.unwrap()["saved"]["games"][&id]["name"],"Prepared image");
    assert_eq!(runtime.committed_metadata_for_test()["saved"]["games"][&id]["name"],"Updated");
}

#[test]
fn close_and_counter_exhaustion_release_refresh_waiters_without_publication() {
    let source = Arc::new(Source::default());let waiter = source.clone();
    let (send,receive) = mpsc::channel();
    let task = thread::spawn(move || { waiter.refresh(true);send.send(()).unwrap(); });
    while source.pending().is_none() { thread::yield_now(); }
    source.close(None);receive.recv_timeout(Duration::from_secs(1)).unwrap();task.join().unwrap();
    let source = Source::default();source.state.lock().unwrap().requested = u64::MAX;
    assert!(source.request(true,None).is_none());
    assert!(source.error().unwrap().contains("exhausted"));source.refresh(true);
}
