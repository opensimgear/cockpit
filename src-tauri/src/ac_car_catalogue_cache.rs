//! Setup-owned installed car choices. This cache never establishes a live car.
use serde_json::{json, Value};
use std::{fs, io::Read, path::PathBuf};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_CARS: usize = 1024;

pub(crate) struct Cache {
    path: PathBuf,
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

impl Cache {
    pub(crate) fn new(path: PathBuf) -> Self {
        let saved = (|| {
            let mut bytes = vec![];
            fs::File::open(&path)
                .ok()?
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() as u64 > MAX_BYTES {
                return None;
            }
            let saved: Value = serde_json::from_slice(&bytes).ok()?;
            if saved["schemaVersion"] != 1 || !valid_cars(&saved["cars"]) {
                return None;
            }
            let game = json!({"id":"assetto-corsa","installed":true,"installPath":saved["installation"]["installPath"],"executable":saved["installation"]["executable"]});
            if scope(&game)? != saved["installation"] {
                return None;
            }
            Some(saved)
        })();
        Self { path, saved }
    }

    pub(crate) fn replace(&mut self, game: &Value, cars: Vec<Value>) -> Result<(), String> {
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
        crate::dcs_setup::replace(&self.path, &bytes)?;
        self.saved = Some(saved);
        Ok(())
    }

    pub(crate) fn decorate(&self, game: &mut Value) {
        if game["id"] != "assetto-corsa" {
            return;
        }
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
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("cars.json");
        let mut cache = Cache::new(path.clone());
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
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("cars.json");
        let mut cache = Cache::new(path.clone());
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
        assert!(Cache::new(path).saved.is_none());
    }
}
