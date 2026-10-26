use super::*;
impl GameIntegrationSetup {
    /// Full provider metadata remains available for a closed game. Reading never
    /// launches a process, opens telemetry or imports an alternate binding store.
    pub(crate) fn resource_integration_catalogue(&self, id: &str) -> Result<Value, String> {
        self.resource_integration_catalogue_for_vehicle(id, None)
    }
    /// Independent authoring captures never change the saved provider module or
    /// the current native aircraft. The Base capture contains CommonData only.
    pub(crate) fn resource_integration_catalogue_for_vehicle(
        &self,
        id: &str,
        vehicle: Option<&str>,
    ) -> Result<Value, String> {
        if crate::game_discovery::integration_provider(id).is_none() {
            return Err("Choose an available telemetry integration.".into());
        }
        let (record, revision) = self.provider_snapshot(id);
        let module = if id == "dcs" {
            validate_connection(id, &record["connection"])?;
            let path = Path::new(
                record["connection"]["cataloguePath"]
                    .as_str()
                    .ok_or("Choose a DCS-BIOS catalogue folder.")?,
            );
            Some(match vehicle {
                Some(vehicle) => crate::dcs_profile::resolve_aircraft_module(path, vehicle)?,
                None => "CommonData".into(),
            })
        } else {
            None
        };
        let key = module
            .as_ref()
            .map(|module| format!("dcs:{module}"))
            .unwrap_or_else(|| id.into());
        let cached = {
            let cache = self
                .inner
                .catalogues
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            cache
                .get(&key)
                .filter(|(at, known, result)| {
                    known == &revision
                        && if id.starts_with("x-plane-") {
                            result.as_ref().is_ok_and(|value| {
                                value["sdkSourceStamp"] == sdk_stamp(id, &record)
                            }) || at.elapsed() < Duration::from_secs(2)
                        } else if id.starts_with("assetto-corsa") {
                            result.is_ok() || at.elapsed() < Duration::from_secs(2)
                        } else {
                            at.elapsed() < Duration::from_secs(2)
                        }
                })
                .map(|(_, _, result)| result.clone())
        };
        if let Some(cached) = cached {
            return cached.map(|value| value.as_ref().clone());
        }
        let result = (|| {
            if let Some(error) = &self.inner.settings_error {
                return Err(error.clone());
            }
            let mut value = match id {
                "dcs" => {
                    validate_connection(id, &record["connection"])?;
                    let mut query = record["connection"].clone();
                    query["aircraftModule"] = json!(module.as_ref().unwrap());
                    let mut value = crate::dcs_profile::resource_catalogue(&query)?;
                    value["channels"] = json!(crate::dcs_physics::catalogue());
                    if let Some(vehicle) = vehicle {
                        value["vehicleId"] = json!(module.as_ref().unwrap());
                        value["vehicleAliases"] = json!(crate::dcs_profile::aircraft_aliases(
                            Path::new(query["cataloguePath"].as_str().unwrap()),
                            module.as_ref().unwrap()
                        )?);
                        if Some(vehicle) != module.as_deref()
                            && !value["vehicleAliases"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|alias| alias == vehicle)
                        {
                            return Err(
                                "The DCS aircraft alias changed during catalogue capture.".into()
                            );
                        }
                    }
                    value
                }
                "x-plane-11" | "x-plane-12" => match record["executable"].as_str() {
                    Some(executable) if Path::new(executable).is_file() => {
                        crate::xplane::catalogue_for_installation(Path::new(executable))?
                    }
                    Some(_) => {
                        let mut value = crate::xplane::catalogue();
                        value["catalogueWarning"]=json!("The selected installation is unavailable. Only legacy channel definitions can be shown until it is accessible.");
                        value
                    }
                    None => crate::xplane::catalogue(),
                },
                _ => crate::game_telemetry::catalogue_for_game(id),
            };
            crate::game_channels::catalogue(id, &mut value);
            if id.starts_with("x-plane-") {
                value["sdkSourceStamp"] = sdk_stamp(id, &record);
            }
            value["catalogueRevision"] =
                json!(digest(&json!(["game-provider-catalogue-v1", id, value])));
            value["providerSetupRevision"] = json!(revision);
            Ok(value)
        })();
        if self.provider_setup_revision(id) != revision {
            return Err("The provider settings changed during catalogue capture; refresh the aircraft catalogue.".into());
        }
        let cached = result.map(Arc::new);
        self.inner
            .catalogues
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, (Instant::now(), revision, cached.clone()));
        cached.map(|value| value.as_ref().clone())
    }
}
