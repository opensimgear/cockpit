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
        source: dcs_bios::decoded::Source,
        decoder: dcs_profile::ExportSession,
        guard: dcs_profile::DcsCatalogueGuard,
        connection: Value,
        module: String,
        resolved_aircraft: Option<String>,
        catalogue: Value,
        physics: Option<crate::dcs_physics::Source>,
        physics_retry: Instant,
        physics_error: Option<String>,
        capture_dirty: bool,
    },
    Rally {
        source: Box<dyn game_telemetry::Source>,
        decoder: game_telemetry::Session,
    },
    Xplane(crate::xplane::Source),
}

pub(super) enum CaptureWatch {
    #[cfg(test)]
    Fixture(Arc<std::sync::atomic::AtomicBool>),
    Assetto { _watch: crate::acr_windows::Watch },
    Xplane {
        _watch: osg_streams::NativeWatch<crate::xplane::NativeSnapshot, crate::xplane::NativeSnapshot>,
        monitor: Option<std::sync::Weak<crate::xplane::process_events::Monitor>>,
    },
    Dcs {
        _bios: osg_streams::NativeWatch<dcs_bios::decoded::Snapshot, dcs_bios::decoded::Snapshot>,
        _ownship: Option<osg_streams::NativeWatch<crate::dcs_physics::Snapshot, crate::dcs_physics::Snapshot>>,
    },
}
impl CaptureWatch {
    pub(super) fn retained_process_current(&self) -> Option<bool> {
        match self {
            Self::Assetto { _watch } => _watch.process_current(),
            Self::Xplane { _watch, monitor } => {
                let monitor = monitor.as_ref()?;
                Some(!_watch.is_closed() && monitor.upgrade().is_some_and(|monitor| monitor.error().is_none()))
            }
            #[cfg(test)]
            Self::Fixture(current) => Some(current.load(std::sync::atomic::Ordering::Acquire)),
            _ => None,
        }
    }
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
                let source = dcs_bios::decoded::Source::open(&network, decoder.decode_spec()?)?;
                let (physics, physics_error) = match crate::dcs_physics::Source::open() {
                    Ok(source) => (Some(source), None),
                    Err(error) => (None, Some(error)),
                };
                Ok(Self::Dcs {
                    source,
                    decoder,
                    guard,
                    connection: connection.clone(),
                    module: module.clone(),
                    resolved_aircraft: None,
                    catalogue,
                    physics,
                    physics_error,
                    capture_dirty: false,
                    physics_retry: Instant::now() + Duration::from_secs(2),
                })
            }
            "assetto-corsa-rally"
            | "assetto-corsa"
            | "assetto-corsa-competizione"
            | "assetto-corsa-evo" => {
                let game = game_telemetry::Game::from_id(id)?;
                Ok(Self::Rally {
                    source: Box::new(crate::acr_windows::SharedSource::open_latest(process, game)?),
                    decoder: game_telemetry::Session::new_for_game(
                        connection["freshnessMs"].as_u64().unwrap_or(1500),
                        game,
                    )?,
                })
            }
            "x-plane-11" | "x-plane-12" => {
                let mut connection = connection.clone();
                connection["processIdentity"] = process.value();
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
    pub(super) fn capture_watch(&self, wake: Arc<osg_streams::NativeWake>) -> Result<Option<CaptureWatch>, String> {
        match self {
            Self::Rally { source, .. } => source.watch(wake)
                .map(|watch| watch.map(|watch| CaptureWatch::Assetto { _watch: watch })),
            Self::Xplane(source) => source.watch(wake)
                .map(|watch| Some(CaptureWatch::Xplane { _watch: watch, monitor: source.process_monitor() })),
            Self::Dcs { source, physics, .. } => Ok(Some(CaptureWatch::Dcs {
                _bios: source.watch(wake.clone())?,
                _ownship: physics.as_ref().and_then(|physics| physics.view().watch(wake).ok()),
            })),
        }
    }
    pub(super) fn take_capture_change(&mut self) -> bool {
        match self {
            Self::Dcs { capture_dirty, .. } => std::mem::take(capture_dirty),
            _ => false,
        }
    }
    pub(super) fn assetto_deadline(&self) -> Option<Instant> {
        match self {
            Self::Rally { decoder, .. } => decoder.capture_deadline(),
            _ => None,
        }
    }
    pub(super) fn ownship_retry_deadline(&self) -> Option<Instant> {
        match self {
            Self::Dcs { physics: None, physics_retry, .. } => Some(*physics_retry),
            _ => None,
        }
    }
    pub(super) fn expire_assetto(&mut self, now_ms: u64) -> Result<(Option<String>, Capture), String> {
        let Self::Rally { decoder, .. } = self else {
            return Err("Only an Assetto reader has a local capture expiry.".into());
        };
        Ok((None, Capture::assetto(&decoder.tick(now_ms))))
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
                resolved_aircraft,
                catalogue,
                physics,
                physics_retry,
                physics_error,
                capture_dirty,
            } => {
                guard.validate(false)?;
                decoder.accept_shared(source.snapshot()?)?;
                let snapshot = decoder.snapshot(now_ms);
                if let Some(aircraft) = snapshot.aircraft.as_ref()
                    .filter(|aircraft| snapshot.fresh && resolved_aircraft.as_ref() != Some(*aircraft))
                {
                    let path = Path::new(connection["cataloguePath"].as_str().unwrap());
                    let current = dcs_profile::resolve_aircraft_module(path, aircraft)?;
                    *resolved_aircraft = Some(aircraft.clone());
                    if current != *module {
                        let next = dcs_profile::passive_telemetry_sources(
                            path,
                            &current,
                            connection["freshnessMs"].as_u64().unwrap_or(1500),
                        )?;
                        *source = dcs_bios::decoded::Source::open(&dcs_bios::Network::parse(connection)?, next.0.decode_spec()?)?;
                        *decoder = next.0;
                        *guard = next.1;
                        *catalogue = next.2;
                        *module = current;
                        *capture_dirty = true;
                        let mut capture = Capture::export("dcs", &snapshot);
                        capture.status = "waiting".into();
                        capture.values.clear();
                        return Ok((None, capture));
                    }
                }
                if physics.as_ref().is_some_and(crate::dcs_physics::Source::is_finished) {
                    *physics_error = physics.as_ref().and_then(|source| source.snapshot().error);
                    // The completed worker has already released its socket.
                    // Preserve BIOS while retrying this independent failed owner.
                    *physics = None;
                    *capture_dirty = true;
                    *physics_retry = Instant::now() + Duration::from_secs(2);
                }
                if physics.is_none() && Instant::now() >= *physics_retry {
                    match crate::dcs_physics::Source::open() {
                        Ok(source) => {
                            *physics = Some(source);
                            *capture_dirty = true;
                            *physics_error = None;
                        }
                        Err(error) => *physics_error = Some(error),
                    }
                    *physics_retry = Instant::now() + Duration::from_secs(2);
                }
                let mut capture = Capture::export("dcs", &snapshot);
                let ownship = physics.as_ref().map(crate::dcs_physics::Source::snapshot);
                if let Some(snapshot) = &ownship { *physics_error = snapshot.error.clone(); }
                if !snapshot.fresh {
                    capture.stale_ids.extend(crate::game_telemetry_inspection::channels(catalogue)
                        .into_iter().filter_map(|channel| channel["id"].as_str().map(str::to_owned)));
                }
                let current = dcs_aircraft(&snapshot, ownship.as_ref(), Instant::now())
                    .0.map(str::to_owned);
                merge_ownship_capture(
                    &mut capture,
                    &snapshot,
                    ownship.as_ref(),
                    physics_error.as_deref(),
                );
                Ok((current, capture))
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
                let snapshot = source.snapshot()?.0;
                Ok((
                    snapshot.aircraft.clone().filter(|_| snapshot.fresh),
                    Capture::export("x-plane-12", &snapshot),
                ))
            }
        }
    }
}
/// Fresh BIOS identifies aircraft changes; otherwise qualified ownship can
/// retain its independent identity and telemetry without granting commands.
pub(super) fn dcs_aircraft<'a>(
    bios: &'a dcs_profile::ExportSnapshot,
    physics: Option<&'a crate::dcs_physics::Snapshot>,
    now: Instant,
) -> (Option<&'a str>, bool) {
    let qualified = physics.filter(|physics| {
        physics.fresh
            && physics.aircraft.as_deref().and_then(|aircraft| vehicle_id("dcs", aircraft)).is_some()
            && physics.captured_at.is_some_and(|at| at <= now && at
                .checked_add(Duration::from_millis(crate::dcs_physics::FRESHNESS_MS + 1))
                .is_some_and(|until| now < until))
            && (!bios.fresh || physics.aircraft == bios.aircraft)
    });
    let aircraft = if bios.fresh { bios.aircraft.as_deref() }
        else { qualified.and_then(|physics| physics.aircraft.as_deref()) };
    (aircraft, qualified.is_some())
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
    let (aircraft, fresh) = dcs_aircraft(bios, physics, Instant::now());
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
        physics.unwrap().captured_values(Instant::now())
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
        if !bios.fresh {
            capture.stale_ids.extend(bios.values.keys().cloned());
            capture.context["aircraft"] = json!(aircraft);
        }
        capture.status = "live".into();
        capture.source = "dcs-bios + ownship".into();
    }
}
