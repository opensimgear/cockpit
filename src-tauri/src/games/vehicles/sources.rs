use super::*;
pub(super) fn dcs_aliases(connection: &Value) -> Result<Value, String> {
    let path = Path::new(
        connection["cataloguePath"]
            .as_str()
            .ok_or("Set up DCS-BIOS to discover aircraft.")?,
    );
    let aliases = dcs_bios::read_json(&path.join("AircraftAliases.json"), dcs_bios::MAX_DOCUMENT)?;
    let object = aliases
        .as_object()
        .filter(|object| object.len() <= MAX_VEHICLES)
        .ok_or("Invalid DCS aircraft aliases catalogue.")?;
    if object.iter().any(|(id, modules)| {
        (!matches!(id.as_str(), "" | "NONE") && vehicle_id("dcs", id).is_none())
            || modules.as_array().is_none_or(|modules| {
                modules.len() > 32 || modules.iter().any(|module| module.as_str().is_none())
            })
    }) {
        return Err("Invalid DCS aircraft aliases catalogue.".into());
    }
    Ok(aliases)
}
pub(super) fn dcs_catalogue(connection: &Value) -> Result<Vec<Value>, String> {
    Ok(dcs_aliases(connection)?
        .as_object()
        .unwrap()
        .keys()
        .filter_map(|id| vehicle("dcs", id))
        .collect())
}
pub(super) fn xplane_catalogue(game: &Value) -> Vec<Value> {
    let Some(root) = game["installPath"].as_str().map(Path::new) else {
        return vec![];
    };
    let mut result = vec![];
    let mut pending = vec![(root.join("Aircraft"), 0)];
    let mut visited = 0;
    while let Some((directory, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 20_000 || result.len() >= MAX_VEHICLES {
                return result;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && depth < 12 {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("acf"))
            {
                if let Ok(relative) = entry.path().strip_prefix(root) {
                    if let Some(value) = vehicle(
                        crate::game_discovery::integration_key(game).unwrap_or(""),
                        &relative.to_string_lossy(),
                    ) {
                        result.push(value);
                    }
                }
            }
        }
    }
    result
}

pub(super) enum Reader {
    Dcs {
        source: dcs_bios::Transport,
        decoder: dcs_profile::ExportSession,
        guard: dcs_profile::DcsCatalogueGuard,
        connection: Value,
        module: String,
        catalogue: Value,
        physics: Option<crate::dcs_physics::Source>,
        physics_retry: Instant,
        physics_error: Option<String>,
    },
    Rally {
        source: Box<dyn game_telemetry::Source>,
        decoder: game_telemetry::Session,
    },
    Xplane(crate::xplane::Source),
}
impl Reader {
    pub(super) fn open(
        id: &str,
        connection: &Value,
        process: &ProcessIdentity,
    ) -> Result<Self, String> {
        match id {
            "dcs" => {
                let aliases = dcs_aliases(connection)?;
                let names = aliases.as_object().unwrap();
                let module = names
                    .iter()
                    .find(|(id, modules)| {
                        !id.is_empty()
                            && modules.as_array().is_some_and(|modules| {
                                modules.iter().any(|module| module == id.as_str())
                            })
                    })
                    .map(|(id, _)| id)
                    .ok_or("DCS aircraft catalogue is empty.")?;
                let path = Path::new(connection["cataloguePath"].as_str().unwrap());
                let network = dcs_bios::Network::parse(connection)?;
                let (decoder, guard, catalogue) =
                    dcs_profile::passive_telemetry_sources(path, module, network.freshness_ms)?;
                let (physics, physics_error) = match crate::dcs_physics::Source::open() {
                    Ok(source) => (Some(source), None),
                    Err(error) => (None, Some(error)),
                };
                Ok(Self::Dcs {
                    source: dcs_bios::Transport::open_network(&network)?,
                    decoder,
                    guard,
                    connection: connection.clone(),
                    module: module.clone(),
                    catalogue,
                    physics,
                    physics_error,
                    physics_retry: Instant::now() + Duration::from_secs(2),
                })
            }
            "assetto-corsa-rally"
            | "assetto-corsa"
            | "assetto-corsa-competizione"
            | "assetto-corsa-evo" => {
                let image = process
                    .path
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&process.path);
                let game = game_telemetry::Game::from_id(id)?;
                Ok(Self::Rally {
                    source: Box::new(crate::acr_windows::WindowsSource::new_for_game(
                        process.pid,
                        image,
                        game,
                    )?),
                    decoder: game_telemetry::Session::new_for_game(
                        connection["freshnessMs"].as_u64().unwrap_or(1500),
                        game,
                    )?,
                })
            }
            "x-plane-11" | "x-plane-12" => {
                let mut connection = connection.clone();
                connection["processIdentity"] = process.value();
                // A separate ephemeral return port avoids taking over a profile's subscriptions.
                connection["listenPort"] = json!(0);
                Ok(Self::Xplane(crate::xplane::Source::new(
                    &connection,
                    vec![],
                )?))
            }
            _ => Err("Vehicle introspection is unavailable for this game.".into()),
        }
    }
    #[cfg(test)]
    pub(super) fn poll(&mut self, now_ms: u64) -> Result<Option<String>, String> {
        self.poll_telemetry(now_ms).map(|(vehicle, _)| vehicle)
    }
    pub(super) fn inspect(&mut self, game: &str, ids: &[String]) -> Result<(), String> {
        if let Self::Xplane(source) = self {
            source.set_inspection_channels(&crate::game_channels::native_ids(game, ids))?;
        }
        Ok(())
    }
    pub(super) fn poll_telemetry(
        &mut self,
        now_ms: u64,
    ) -> Result<(Option<String>, Capture), String> {
        match self {
            Self::Dcs {
                source,
                decoder,
                guard,
                connection,
                module,
                catalogue,
                physics,
                physics_retry,
                physics_error,
            } => {
                guard.validate(false)?;
                let mut drained = false;
                for _ in 0..64 {
                    let Some(bytes) = source.receive()? else {
                        drained = true;
                        break;
                    };
                    decoder.feed(&bytes, now_ms);
                }
                if !drained && source.receive()?.is_some() {
                    decoder.reset_stream(now_ms);
                    return Err("DCS vehicle observation backlog exceeded its bound.".into());
                }
                decoder.tick(now_ms);
                let snapshot = decoder.snapshot(now_ms);
                if let Some(aircraft) = snapshot.aircraft.as_ref().filter(|_| snapshot.fresh) {
                    let path = Path::new(connection["cataloguePath"].as_str().unwrap());
                    let current = dcs_profile::resolve_aircraft_module(path, aircraft)?;
                    if current != *module {
                        let next = dcs_profile::passive_telemetry_sources(
                            path,
                            &current,
                            connection["freshnessMs"].as_u64().unwrap_or(1500),
                        )?;
                        *decoder = next.0;
                        *guard = next.1;
                        *catalogue = next.2;
                        *module = current;
                        let mut capture = Capture::export("dcs", &snapshot);
                        capture.status = "waiting".into();
                        capture.values.clear();
                        return Ok((None, capture));
                    }
                }
                if physics.is_none() && Instant::now() >= *physics_retry {
                    match crate::dcs_physics::Source::open() {
                        Ok(source) => {
                            *physics = Some(source);
                            *physics_error = None;
                        }
                        Err(error) => *physics_error = Some(error),
                    }
                    *physics_retry = Instant::now() + Duration::from_secs(2);
                }
                let mut capture = Capture::export("dcs", &snapshot);
                let ownship = match physics.as_mut().map(|physics| physics.poll()) {
                    Some(Ok(snapshot)) => {
                        *physics_error = snapshot.error.clone();
                        Some(snapshot)
                    }
                    Some(Err(error)) => {
                        *physics = None;
                        *physics_retry = Instant::now() + Duration::from_secs(2);
                        *physics_error = Some(error);
                        None
                    }
                    None => None,
                };
                merge_ownship_capture(
                    &mut capture,
                    &snapshot,
                    ownship.as_ref(),
                    physics_error.as_deref(),
                );
                Ok((
                    snapshot.aircraft.clone().filter(|_| snapshot.fresh),
                    capture,
                ))
            }
            Self::Rally { source, decoder } => {
                let snapshot = decoder.poll(source.as_mut(), now_ms);
                if snapshot.status == "error" {
                    return Err(snapshot
                        .error
                        .unwrap_or_else(|| "Assetto vehicle telemetry failed.".into()));
                }
                let capture = Capture::assetto(&snapshot);
                Ok((
                    snapshot
                        .context
                        .filter(|_| snapshot.fresh)
                        .map(|context| context.car_model),
                    capture,
                ))
            }
            Self::Xplane(source) => {
                let snapshot = source.poll(Instant::now()).0;
                Ok((
                    snapshot.aircraft.clone().filter(|_| snapshot.fresh),
                    Capture::export("x-plane-12", &snapshot),
                ))
            }
        }
    }
}
pub(super) fn merge_ownship_capture(
    capture: &mut Capture,
    bios: &dcs_profile::ExportSnapshot,
    physics: Option<&crate::dcs_physics::Snapshot>,
    error: Option<&str>,
) {
    static IDS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let ids = IDS.get_or_init(|| {
        let mut catalogue = json!({"channels":crate::dcs_physics::catalogue()});
        crate::game_channels::catalogue("dcs", &mut catalogue);
        catalogue["channels"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|field| field["id"].as_str().map(str::to_owned))
            .collect()
    });
    let fresh = bios.fresh
        && physics.is_some_and(|physics| physics.fresh && physics.aircraft == bios.aircraft);
    capture.context["physicsStatus"] = json!(if error.is_some() {
        "error"
    } else if physics.is_some_and(|physics| physics.fresh) && !fresh {
        "waiting-aircraft"
    } else {
        physics.map_or("waiting", |physics| physics.status.as_str())
    });
    capture.context["physicsAgeMs"] = json!(physics.and_then(|physics| physics.age_ms));
    if let Some(error) = error {
        let error = error
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect::<String>();
        capture.context["physicsError"] = json!(error);
        capture.message = Some(format!("DCS ownship telemetry: {error}"));
    }
    let values = if fresh {
        crate::game_channels::values("dcs", &physics.unwrap().values)
    } else {
        BTreeMap::new()
    };
    for id in ids {
        capture.values.remove(id);
        capture.stale_ids.remove(id);
        capture.ages.insert(
            id.clone(),
            if fresh && !values.contains_key(id) {
                None
            } else {
                physics.and_then(|physics| physics.age_ms)
            },
        );
        if !fresh {
            capture.stale_ids.insert(id.clone());
        }
    }
    if fresh {
        capture.values.extend(values);
        capture.source = "dcs-bios + ownship".into();
    }
}
