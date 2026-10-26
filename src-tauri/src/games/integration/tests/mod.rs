use super::*;
use crate::game_discovery::ProcessIdentity;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("osg-setup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        #[cfg(unix)]
        let path = fs::canonicalize(path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct FakePlatform(bool);
impl Platform for FakePlatform {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        Err("Unexpected launch".into())
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(self.0.then(|| ProcessIdentity {
            pid: 42,
            path: "DCS.exe".into(),
            epoch: 1,
        }))
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        Ok(self.0)
    }
}
fn game() -> Value {
    json!({"id":"dcs","integrationCompatible":true,"integrationKey":"dcs","provider":"dcs-bios","installed":true})
}
fn wait_setup(runtime: &GameIntegrationSetup) {
    // These fixtures verify completion, not setup latency. Instrumented suites
    // run filesystem sync and recovery work alongside all native tests.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while runtime.busy() {
        if std::time::Instant::now() >= end {
            let states = runtime.inner.states.try_lock().map(|states| states.clone());
            let worker_finished = runtime
                .worker
                .try_lock()
                .map(|worker| worker.as_ref().map(JoinHandle::is_finished));
            assert!(
                !runtime.busy(),
                "Setup did not finish within 30 seconds; pending={}, worker_finished={worker_finished:?}, states={states:?}",
                runtime.pending()
            );
            break;
        }
        thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn decorate(runtime: &GameIntegrationSetup, catalogue: &mut [Value]) {
    runtime.decorate(catalogue);
}
fn acr_game(scratch: &Scratch) -> Value {
    let executable = scratch.0.join("acr.exe");
    fs::write(
        &executable,
        b"Authored executable identity fixture, never launched",
    )
    .unwrap();
    json!({"id":"assetto-corsa-rally","integrationCompatible":true,"integrationKey":"assetto-corsa-rally","provider":"assetto-corsa-rally","installed":true,"executable":executable})
}
fn acr_record(runtime: &GameIntegrationSetup, game: &Value, qualified: bool) -> Value {
    let identity =
        crate::acr_setup::fingerprint(Path::new(game["executable"].as_str().unwrap())).unwrap();
    let record = json!({"connection":{"source":"shared-memory","freshnessMs":1700},"configuredFingerprint":identity,"verifiedFingerprint":if qualified {identity.clone()} else {Value::Null}});
    runtime.inner.saved.lock().unwrap()["games"]["assetto-corsa-rally"] = record;
    runtime.refresh(&[game.clone()]);
    identity
}
fn ac_catalogue_game(root: &Path) -> Value {
    fs::create_dir_all(root.join("content/cars")).unwrap();
    let executable = root.join("acs.exe");
    fs::write(&executable, b"Unlaunched AC car catalogue setup fixture").unwrap();
    json!({"id":"assetto-corsa","integrationCompatible":true,"integrationKey":"assetto-corsa","provider":"assetto-corsa","installed":true,
        "installPath":root,"executable":executable,"currentVehicle":null,
        "vehicleSupport":{"vehicles":[]}})
}

fn ac_catalogue_car(root: &Path, id: &str, name: &str) {
    let car = root.join("content/cars").join(id);
    fs::create_dir_all(car.join("ui")).unwrap();
    fs::create_dir_all(car.join("skins/default")).unwrap();
    fs::write(car.join("ui/ui_car.json"), json!({"name":name}).to_string()).unwrap();
    fs::write(
        car.join("data.acd"),
        b"Authored data archive presence fixture",
    )
    .unwrap();
    fs::write(
        car.join("skins/default/livery.dds"),
        b"Authored skin presence fixture",
    )
    .unwrap();
}

fn ac_catalogue_choices(runtime: &GameIntegrationSetup, game: &Value) -> Value {
    let mut decorated = game.clone();
    runtime.decorate_installed_cars(&mut decorated);
    assert_eq!(
        decorated["currentVehicle"],
        Value::Null,
        "Installed choices cannot establish a live car"
    );
    decorated["vehicleSupport"]["vehicles"].clone()
}

mod assetto;
mod baseline;
mod dcs;
