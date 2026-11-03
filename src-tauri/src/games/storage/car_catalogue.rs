//! Setup-owned installed car choices. This cache never establishes a live car.
use serde_json::{json, Value};
#[cfg(test)]
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_CARS: usize = 1024;

pub(crate) struct Cache {
    store: Result<osg_persistence::Repository<Value>, String>,
    writer: Mutex<()>,
    snapshot: Mutex<Arc<Snapshot>>,
    #[cfg(test)]
    pub(crate) commit_pause:
        Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
}

pub(crate) struct Snapshot {
    error: Option<String>,
    saved: Option<Value>,
}

fn path_key(value: &Value) -> Option<String> {
    let text = value.as_str().filter(|text| {
        !text.is_empty() && text.len() <= 4096 && !text.chars().any(char::is_control)
    })?;
    if !std::path::Path::new(text).is_absolute() {
        return None;
    }
    let text = text.replace('\\', "/");
    let text = if text
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("//?/UNC/"))
    {
        format!("//{}", &text[8..])
    } else {
        text.strip_prefix("//?/").unwrap_or(&text).to_owned()
    };
    let text = text.trim_end_matches('/');
    Some(if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_owned()
    })
}

fn scope(game: &Value) -> Option<Value> {
    if game["id"] != "assetto-corsa" || game["installed"] != true {
        return None;
    }
    Some(
        json!({"installPath":path_key(&game["installPath"])?,"executable":path_key(&game["executable"])?}),
    )
}

fn valid_cars(cars: &Value) -> bool {
    let Some(cars) = cars.as_array().filter(|cars| cars.len() <= MAX_CARS) else {
        return false;
    };
    let mut ids = std::collections::BTreeSet::new();
    cars.iter().all(|car| {
        let Some(id) = car["id"].as_str() else {
            return false;
        };
        let Some(name) = car["name"].as_str() else {
            return false;
        };
        crate::game_vehicles::vehicle_id("assetto-corsa", id).as_deref() == Some(id)
            && !id.contains(['/', '\\'])
            && !matches!(id, "." | "..")
            && !name.is_empty()
            && name.trim() == name
            && name.len() <= 256
            && !name.chars().any(char::is_control)
            && ids.insert(id)
    })
}

fn validate(saved: &Value) -> Result<(), String> {
    if saved["schemaVersion"] != 1 || !valid_cars(&saved["cars"]) {
        return Err("Invalid saved Assetto Corsa car catalogue; preserve it for recovery.".into());
    }
    let game = json!({"id":"assetto-corsa","installed":true,"installPath":saved["installation"]["installPath"],"executable":saved["installation"]["executable"]});
    if scope(&game).as_ref() != Some(&saved["installation"]) {
        return Err("Invalid saved car installation scope.".into());
    }
    Ok(())
}
impl Cache {
    pub(crate) fn new(path: PathBuf) -> Self {
        let store = osg_persistence::Database::for_namespace(&path).and_then(|db| {
            osg_persistence::Repository::new(db, "games", "ac-cars", MAX_BYTES as usize, validate)
        });
        let loaded = store
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|store| store.load());
        let snapshot = match loaded {
            Ok(saved) => Snapshot { saved, error: None },
            Err(error) => Snapshot {
                saved: None,
                error: Some(error),
            },
        };
        Self {
            store,
            writer: Mutex::new(()),
            snapshot: Mutex::new(Arc::new(snapshot)),
            #[cfg(test)]
            commit_pause: Mutex::new(None),
        }
    }

    pub(crate) fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    #[cfg(test)]
    fn replace(&self, game: &Value, cars: Vec<Value>) -> Result<(), String> {
        self.replace_if(game, cars, || Ok(()))
    }

    pub(crate) fn replace_if(
        &self,
        game: &Value,
        cars: Vec<Value>,
        before_commit: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let installation = scope(game)
            .ok_or("Choose an installed Assetto Corsa installation before scanning cars.")?;
        let cars = json!(cars);
        if !valid_cars(&cars) {
            return Err("The Assetto Corsa car catalogue is invalid or exceeds its limit.".into());
        }
        let saved = json!({"schemaVersion":1,"installation":installation,"cars":cars});
        let bytes = serde_json::to_vec(&saved).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("The Assetto Corsa car catalogue exceeds its size limit.".into());
        }
        let _writer = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        let previous = self.snapshot();
        let store = self.store.as_ref().map_err(Clone::clone)?;
        before_commit()?;
        #[cfg(test)]
        if let Some((entered, release)) = self
            .commit_pause
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            entered
                .send(())
                .map_err(|_| "Catalogue persistence test disconnected.")?;
            release
                .recv_timeout(std::time::Duration::from_secs(15))
                .map_err(|_| "Catalogue persistence test timed out.")?;
        }
        if previous.error.is_some() {
            store.reset(&saved)?;
        } else {
            store.save(&saved)?;
        }
        *self.snapshot.lock().unwrap_or_else(|p| p.into_inner()) = Arc::new(Snapshot {
            error: None,
            saved: Some(saved),
        });
        Ok(())
    }

    #[cfg(test)]
    fn decorate(&self, game: &mut Value) {
        self.snapshot().decorate(game);
    }
}

impl Snapshot {
    pub(crate) fn decorate(&self, game: &mut Value) {
        if game["id"] != "assetto-corsa" {
            return;
        }
        game["vehicleSupport"]["retentionError"] = json!(self.error);
        let selected = scope(game);
        let matching = self
            .saved
            .as_ref()
            .filter(|saved| Some(&saved["installation"]) == selected.as_ref());
        // A completed scan replaces historical choices, including removed cars.
        // A cache for another installation must not leak its installed roster.
        if self.saved.is_some() || game["installed"] != true {
            let mut cars = matching
                .and_then(|saved| saved["cars"].as_array())
                .cloned()
                .unwrap_or_default();
            if let Some(id) = game["currentVehicle"]["id"].as_str() {
                if let Some(car) = cars.iter().find(|car| car["id"] == id) {
                    game["currentVehicle"]["name"] = car["name"].clone();
                } else if game["currentVehicle"].is_object() && selected.is_some() {
                    cars.push(game["currentVehicle"].clone());
                }
            }
            game["vehicleSupport"]["vehicles"] = json!(cars);
        }
        game["vehicleSupport"]["message"] = json!(if matching.is_some() {
            "Installed cars were scanned during integration setup/repair. Run setup/repair again after adding or removing cars. Current car requires fresh native telemetry."
        } else {
            "Run integration setup/repair to scan this installation's cars. Current car requires fresh native telemetry."
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(root: &std::path::Path) -> Value {
        json!({"id":"assetto-corsa","installed":true,"installPath":root,"executable":root.join("acs.exe"),"currentVehicle":null,"vehicleSupport":{"vehicles":[]}})
    }

    #[test]
    fn persists_names_scopes_installations_and_replaces_removed_cars_without_live_identity() {
        let scratch = crate::test_directory();
        let path = scratch.path().join("cars.json");
        let cache = Cache::new(path.clone());
        let mut selected = game(&scratch.path().join("AC"));
        cache
            .replace(
                &selected,
                vec![
                    json!({"id":"lotus_elise_sc","name":"Lotus Elise SC"}),
                    json!({"id":"mod_car","name":"My Mod"}),
                ],
            )
            .unwrap();
        selected["vehicleSupport"]["vehicles"] = json!([{"id":"old_car","name":"old_car"}]);
        let restored = Cache::new(path.clone());
        restored.decorate(&mut selected);
        assert_eq!(
            selected["vehicleSupport"]["vehicles"][0]["name"],
            "Lotus Elise SC"
        );
        assert_eq!(selected["currentVehicle"], Value::Null);
        selected["currentVehicle"] = json!({"id":"lotus_elise_sc","name":"lotus_elise_sc"});
        restored.decorate(&mut selected);
        assert_eq!(selected["currentVehicle"]["name"], "Lotus Elise SC");
        cache
            .replace(
                &selected,
                vec![json!({"id":"lotus_elise_sc","name":"Renamed Lotus"})],
            )
            .unwrap();
        let restored = Cache::new(path);
        restored.decorate(&mut selected);
        assert_eq!(
            selected["vehicleSupport"]["vehicles"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            selected["vehicleSupport"]["vehicles"][0]["name"],
            "Renamed Lotus"
        );
        let mut other = game(&scratch.path().join("Other AC"));
        restored.decorate(&mut other);
        assert_eq!(other["vehicleSupport"]["vehicles"], json!([]));
        other["id"] = json!("assetto-corsa-competizione");
        let before = other.clone();
        restored.decorate(&mut other);
        assert_eq!(other, before);
    }

    #[test]
    fn failed_replacement_keeps_the_previous_snapshot_and_invalid_cache_is_ignored() {
        let scratch = crate::test_directory();
        let path = scratch.path().join("cars.json");
        let cache = Cache::new(path.clone());
        let mut selected = game(scratch.path());
        cache
            .replace(&selected, vec![json!({"id":"car","name":"Car"})])
            .unwrap();
        assert!(cache
            .replace(&selected, vec![json!({"id":"../other","name":"Other"})])
            .is_err());
        cache.decorate(&mut selected);
        assert_eq!(selected["vehicleSupport"]["vehicles"][0]["id"], "car");
        fs::write(path.clone(), b"invalid cache").unwrap();
        assert!(
            Cache::new(path).snapshot().saved.is_some(),
            "retired files cannot replace SQLite"
        );
        let invalid = scratch.path().join("other/cars.json");
        fs::create_dir_all(invalid.parent().unwrap()).unwrap();
        fs::write(&invalid, b"invalid cache").unwrap();
        crate::persistence::test_support::record(&invalid, "games", "ac-cars", b"invalid cache");
        let broken = Cache::new(invalid.clone());
        assert!(broken.snapshot().error.is_some());
        broken
            .replace(&selected, vec![json!({"id":"car","name":"Repaired"})])
            .unwrap();
        assert!(broken.snapshot().error.is_none());
        assert_eq!(fs::read(invalid).unwrap(), b"invalid cache");
    }

    #[test]
    fn persistence_conflict_keeps_the_exact_previous_snapshot_and_external_record() {
        let scratch = crate::test_directory();
        let path = scratch.path().join("cars.json");
        let cache = Cache::new(path.clone());
        let selected = game(scratch.path());
        cache
            .replace(&selected, vec![json!({"id":"car","name":"Original"})])
            .unwrap();
        let previous = cache.snapshot();
        let db = osg_persistence::Database::for_namespace(&path).unwrap();
        let record = db.read("games", "ac-cars").unwrap().unwrap();
        let mut external = previous.saved.clone().unwrap();
        external["cars"][0]["name"] = json!("External writer");
        let payload = serde_json::to_vec(&external).unwrap();
        db.commit(&[osg_persistence::Change {
            domain: "games",
            key: "ac-cars",
            expected_revision: record.revision,
            payload: Some(&payload),
        }])
        .unwrap();
        assert!(cache
            .replace(&selected, vec![json!({"id":"car","name":"Stale writer"})])
            .unwrap_err()
            .contains("revision conflict"));
        assert!(Arc::ptr_eq(&previous, &cache.snapshot()));
        let retained = db.read("games", "ac-cars").unwrap().unwrap();
        assert_eq!(retained.payload, payload);
        assert_eq!(retained.revision, record.revision + 1);
    }
}
