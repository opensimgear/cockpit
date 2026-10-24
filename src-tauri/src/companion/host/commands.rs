use super::*;

impl Runtime {
    pub fn request(&self, request: &Value) -> Result<Value, String> {
        let command = request["command"]
            .as_str()
            .ok_or("Missing companion command.")?;
        if command=="companion_game_capture" {
            return Err("Independent game companion capture is retired; edit the unified profile.".into());
        }
        let payload = request
            .get("companion")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !payload.is_object() {
            return Err("Companion operator payload must be an object.".into());
        }
        let owner = request["clientId"]
            .as_str()
            .ok_or("Missing local operator identity.")?;
        // Pause new ESP32 pairing while its replacement renderer is deferred.
        // Saved screens can still use their existing connection settings flow.
        if (command == "companion_pair_offer" && payload["kind"] == "esp32")
            || (command == "companion_usb_provision"
                && payload["companionId"].as_str().is_none_or(|id| id.trim().is_empty()))
        {
            return Err("ESP32 pairing is temporarily unavailable.".into());
        }
        match command {
            "companion_inventory" => return self.inventory(),
            "companion_publish_begin" => {
                let expected = payload["size"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= MAX_UPLOAD as u64)
                    .ok_or("Panel upload must contain 1â€“13 MiB.")?
                    as usize;
                let mut slot = self.upload.lock().unwrap_or_else(|p| p.into_inner());
                if slot
                    .as_ref()
                    .is_some_and(|u| u.started.elapsed() < Duration::from_secs(60))
                {
                    return Err("Another panel upload is active; finish or abort it first.".into());
                }
                let id = uuid::Uuid::new_v4().to_string();
                *slot = Some(Upload {
                    id: id.clone(),
                    owner: owner.into(),
                    expected,
                    bytes: Vec::new(),
                    started: Instant::now(),
                });
                return Ok(json!({"uploadId":id}));
            }
            "companion_publish_abort" => {
                let mut slot = self.upload.lock().unwrap_or_else(|p| p.into_inner());
                if slot.as_ref().is_some_and(|u| {
                    u.owner == owner && payload["uploadId"].as_str() == Some(&u.id)
                }) {
                    *slot = None;
                }
                return Ok(json!({}));
            }
            "companion_publish_chunk" | "companion_publish_commit" => {
                let mut slot = self.upload.lock().unwrap_or_else(|p| p.into_inner());
                if slot
                    .as_ref()
                    .is_some_and(|u| u.started.elapsed() >= Duration::from_secs(60))
                {
                    *slot = None;
                    return Err("Panel upload expired; start a fresh upload.".into());
                }
                let upload = slot
                    .as_mut()
                    .filter(|u| u.owner == owner && payload["uploadId"].as_str() == Some(&u.id))
                    .ok_or("Panel upload does not belong to this operator.")?;
                if command.ends_with("_chunk") {
                    let bytes = payload["bytes"]
                        .as_array()
                        .filter(|a| !a.is_empty() && a.len() <= 8192)
                        .ok_or("Panel chunks contain 1â€“8192 bytes.")?;
                    if payload["offset"].as_u64() != Some(upload.bytes.len() as u64)
                        || upload.bytes.len() + bytes.len() > upload.expected
                    {
                        return Err("Panel chunk offset or length mismatch.".into());
                    }
                    let chunk = bytes
                        .iter()
                        .map(|v| {
                            v.as_u64()
                                .filter(|v| *v <= 255)
                                .map(|v| v as u8)
                                .ok_or("Invalid panel byte.")
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    upload.bytes.extend_from_slice(&chunk);
                    return Ok(json!({"received":upload.bytes.len()}));
                }
                if upload.bytes.len() != upload.expected {
                    return Err("Panel upload is incomplete.".into());
                }
                let bytes = slot.take().unwrap().bytes;
                drop(slot);
                let mut value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| "Invalid panel upload JSON.")?;
                let object = value
                    .as_object_mut()
                    .ok_or("Panel upload must be an object.")?;
                if !["canonicalDocument", "publicationHash", "assets"]
                        .iter()
                        .all(|k| object.contains_key(*k))
                    || object.keys().any(|key| !["canonicalDocument", "publicationHash", "capabilityFingerprint", "assets"].contains(&key.as_str()))
                {
                    return Err("Unexpected panel upload fields.".into());
                }
                object.insert("command".into(), json!("companion_publish"));
                if payload.get("saveAsLatest").is_some_and(|value|!value.is_boolean()) {
                    return Err("saveAsLatest must be a boolean.".into());
                }
                let services = self.services()?;
                let _cache=services.panel_cache.lock().unwrap_or_else(|p|p.into_inner());
                value["saveAsLatest"]=json!(payload["saveAsLatest"]!=false);
                if let Some(selection)=payload.get("selection"){value["selection"]=selection.clone();}
                else if value["saveAsLatest"]==true {
                    let token=services.hub.command(&json!({"command":"companion_status"}))?["snapshotToken"].clone();
                    value["selection"]=json!({"expectedHostToken":token});
                }
                let published=services.hub.command(&value)?;
                let saved=published.get("panelSaved").cloned();
                Self::start_saved_listener(services);
                let mut response=self.status(services)?;
                if let Some(saved)=saved {response["panelSaved"]=saved;}
                if published["selectionCommitted"]==true {response["selectionCommitted"]=json!(true);}
                return Ok(response);
            }
            _ => {}
        }
        let services = self.services()?;
        match command {
            "companion_firmware_check" => {
                services.firmware_releases.check();
                self.status(services)
            }
            "companion_usb_ports" => {
                let mut status=services.firmware_releases.metadata();
                status["usbPorts"]=services.usb.pairing_ports(&services.hub,payload["companionId"].as_str())?;
                services.firmware_releases.observe_ports(&status["usbPorts"],&services.usb.statuses());
                status["usb"]=services.usb.status();
                Ok(status)
            }
            "companion_usb_update_prepare" => {
                let _setup=services.usb_setup.lock().unwrap_or_else(|p|p.into_inner());
                let id=payload["companionId"].as_str().ok_or("Choose a paired companion.")?;
                let clients=services.hub.command(&json!({"command":"companion_status"}))?;
                if !clients["clients"].as_array().is_some_and(|rows|rows.iter().any(|c|c["companionId"]==id && c["kind"]=="esp32")){return Err("Choose a paired ESP32 companion.".into());}
                let ports=services.usb.pairing_ports(&services.hub,Some(id))?;
                services.firmware_releases.observe_ports(&ports,&services.usb.statuses());
                if ports.as_array().is_none_or(|rows|rows.len()!=1){return Err("Connect only this paired companion by USB, then try again.".into());}
                let metadata=crate::companion_settings::firmware(&services.directory)?.1;
                if !matches!(metadata[id]["profile"].as_str(),Some("sunton-3248s035r"|"waveshare-lcd7-s3")){return Err("This legacy companion has no saved board profile. Identify its board before updating firmware.".into());}
                services.usb.release_port(ports[0]["port"].as_str().ok_or("USB port is unavailable.")?);
                Ok(json!({"usbPorts":ports,"usb":services.usb.status()}))
            }
            "companion_usb_flash_cancel" => {
                services.usb.cancel_firmware_wait()?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_flash" => {
                let _setup = services.usb_setup.lock().unwrap_or_else(|p| p.into_inner());
                let (profile,target)=match payload["profile"].as_str() {
                    Some("waveshare-lcd7-s3")=>("waveshare-lcd7-s3",osg_companion_serial::FirmwareTarget::Esp32s3),
                    Some("sunton-3248s035r")=>("sunton-3248s035r",osg_companion_serial::FirmwareTarget::Esp32),
                    _=>return Err("Choose a supported board profile.".into()),
                };
                let port = payload["port"].as_str().ok_or("Choose a USB device.")?;
                let update=payload["companionId"].as_str();
                if let Some(id)=update{
                    let metadata=crate::companion_settings::firmware(&services.directory)?.1;
                    if metadata[id]["profile"].as_str().is_some_and(|profile|Some(profile)!=payload["profile"].as_str()){return Err("The firmware profile does not match this companion.".into());}
                }
                if !match update {Some(id)=>services.usb.update_candidate_available(port,id)?,None=>services.usb.flash_candidate_available(port)?}
                {
                    return Err("The USB selection changed or firmware detection is still in progress. Refresh devices and select this USB port again.".into());
                }
                let mut installed=Vec::new();
                if let Some(version)=services.firmware_releases.observed_version(port,profile){installed.push(version);}
                if let Some(id)=update {
                    let metadata=crate::companion_settings::firmware(&services.directory)?.1;
                    if let Some(version)=metadata[id]["version"].as_str(){installed.push(version.to_owned());}
                }
                let firmware=services.firmware_releases.load(&services.directory,profile,&installed)?;
                // Downloading must not bypass the serial manager's fresh device identity check.
                if !match update {Some(id)=>services.usb.update_candidate_available(port,id)?,None=>services.usb.flash_candidate_available(port)?} {
                    return Err("The USB selection changed while downloading firmware. Refresh devices and select this USB port again.".into());
                }
                services.usb.flash_owned_options_preserving(
                    payload["port"].as_str().ok_or("Choose a USB device.")?,
                    firmware, target, true, update.is_some(),
                )?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_status" => Ok(json!({"usb":services.usb.status()})),
            "companion_usb_connect" => {
                let port = payload["port"].as_str().ok_or("Select a USB device.")?;
                services.usb.attach(port, services.hub.clone())?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_disconnect" => {
                services.usb.release();
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_scan" => {
                services.usb.scan()?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_arm" => {
                services.usb.arm()?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_disarm" | "companion_usb_forget" | "companion_usb_reconnect" => {
                let op = match command {
                    "companion_usb_disarm" => "disarm",
                    "companion_usb_forget" => "forget",
                    _ => "connect",
                };
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| "System clock is invalid.")?
                    .as_millis() as u64;
                services.usb.record_for(payload["companionId"].as_str(),json!({"command":op,"unixMs":ms}))?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_usb_provision" => {
                let _setup = services.usb_setup.lock().unwrap_or_else(|p| p.into_inner());
                if services.usb.status()["provisioning"] == true {
                    return Err("Device setup is already running.".into());
                }
                if services.usb.status()["ready"] != true {
                    return Err("Wait for the USB device to be ready.".into());
                }
                let usb_status=services.usb.status();
                let mut client=json!({"kind":"esp32","connected":true,"renderProfile":usb_render_profile(&usb_status)});
                if !usb_status["device"]["renderProfile"].is_null()&&client["renderProfile"].is_null(){client["renderProfileError"]=json!("The USB renderer report does not match its firmware identity.");}
                let firmware=json!({"version":usb_status["device"]["firmwareVersion"],"profile":usb_status["device"]["boardProfile"]});
                if let Some(hash)=payload["publicationHash"].as_str().filter(|hash|!hash.is_empty()){validate_target_panel(&services.hub,hash,&client,&firmware)?;}
                let name = payload["name"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 80)
                    .ok_or("Enter a device name.")?;
                let mode = payload["mode"]
                    .as_str()
                    .filter(|s| *s == "wifi" || *s == "serial")
                    .ok_or("Choose Wi-Fi or direct USB.")?;
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| "System clock is invalid.")?
                    .as_millis() as u64;
                let status = services.usb.status();
                let host = services
                    .hub
                    .command(&json!({"command":"companion_status"}))?;
                let existing = status["device"]["companionId"].as_str().filter(|id| {
                    host["clients"]
                        .as_array()
                        .is_some_and(|rows| rows.iter().any(|c| c["companionId"] == *id && c["kind"] == "esp32"))
                });
                let id = if let Some(selected) = payload["companionId"].as_str() {
                    if existing != Some(selected) {
                        return Err("Connect the selected paired screen before changing its connection settings.".into());
                    }
                    selected.to_owned()
                } else {
                    if existing.is_some() {
                        return Err(
                            "This screen is already paired. Open its settings instead.".into()
                        );
                    }
                    uuid::Uuid::new_v4().to_string()
                };
                let offer = services
                    .hub
                    .command(&json!({"command":"companion_pair_offer","kind":"esp32"}))?;

                let mut record = if mode == "wifi" {
                    let ssid = payload["ssid"]
                        .as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 32)
                        .ok_or("Choose a Wi-Fi network.")?;
                    let password = payload["password"]
                        .as_str()
                        .filter(|s| s.len() <= 64)
                        .ok_or("Enter the Wi-Fi password.")?;
                    let pair = services.server.pairing_payload(
                        offer["secret"].as_str().ok_or("Missing offer.")?,
                        offer["expiresAtMs"]
                            .as_u64()
                            .ok_or("Missing offer expiry.")?,
                    )?;
                    json!({"command":"provision","ssid":ssid,"password":password,"keepWifiPassword":payload["keepWifiPassword"]==true,"pairing":pair,"unixMs":ms,"persist":true})
                } else {
                    json!({"command":"provision-direct","hostId":offer["hostId"],"secret":offer["secret"]})
                };
                record["name"] = json!(name);
                record["companionId"] = json!(id);
                if let Some(hash) = payload["publicationHash"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                {
                    record["_assignment"] = json!({"publicationHash":hash,"instanceId":uuid::Uuid::new_v4().to_string()});
                }
                services.usb.provision(record, id)?;
                Ok(json!({"usb":services.usb.status()}))
            }
            "companion_status" => self.status(services),
            "companion_enable" => {
                let address: IpAddr = payload["address"]
                    .as_str()
                    .ok_or("Choose a listen IP address.")?
                    .parse()
                    .map_err(|_| "Listen address must be an IP address.")?;
                let port = payload["port"]
                    .as_u64()
                    .filter(|p| (1024..=65535).contains(p))
                    .ok_or("Choose a port from 1024 to 65535.")? as u16;
                let advertised_host = payload["advertisedHost"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 253)
                    .ok_or("Choose the address companions can reach.")?
                    .to_owned();
                let config=ServerConfig {
                    bind: SocketAddr::new(address, port),
                    advertised_host,
                    discovery: payload["discovery"] == true,
                };
                if services.server.status().enabled{return Err("Companion listener is already enabled.".into());}
                services.saved_listener.save(Some(config),payload.get("expectedSettingsRevision").map(|v|v.as_u64().ok_or("Invalid listener revision.")).transpose()?)?;
                Self::start_saved_listener(services);
                self.status(services)
            }
            "companion_disable" => {
                services.saved_listener.save_with_effect(None,payload.get("expectedSettingsRevision").map(|v|v.as_u64().ok_or("Invalid listener revision.")).transpose()?,||services.server.stop())?;
                self.status(services)
            }
            "companion_browser_certificate" => Ok(json!({"certificatePem":services.server.browser_certificate()?})),
            "companion_pair_offer" => {
                if payload["kind"] == "browser" {
                    let browser = services.server.browser_url()?;
                    let offer=services.hub.command(&json!({"command":"companion_pair_offer","kind":"browser","autoApprove":true}))?;
                    let url=format!("{}#pair={}",browser,offer["secret"].as_str().ok_or("Missing pairing secret")?);
                    let mut state=self.status(services)?;
                    state["pairing"]=json!({"browserUrl":url,"ttlMs":60_000,"qrMatrix":[],"manualPayload":""});
                    return Ok(state);
                }
                let offer = services
                    .hub
                    .command(&json!({"command":"companion_pair_offer","kind":payload["kind"].as_str().unwrap_or("desktop"),"autoApprove":true}))?;
                let secret = offer["secret"]
                    .as_str()
                    .ok_or("Pairing offer did not contain a secret.")?;
                let expires = offer["expiresAtMs"]
                    .as_u64()
                    .ok_or("Pairing offer did not contain an expiry.")?;
                let payload = services.server.pairing_payload(secret, expires)?;
                let code = services
                    .hub
                    .command(&json!({"command":"companion_pair_code","payload":payload}))?;
                let manual = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
                // Dense binary rows preserve every QR module without a boolean
                // JSON array exceeding the bounded private-service reply frame.
                let matrix: Vec<String> = qrcode_matrix(&manual)?
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|dark| if dark { '1' } else { '0' })
                            .collect()
                    })
                    .collect();
                let mut state = self.status(services)?;
                state["pairing"] = json!({"manualPayload":manual,"qrMatrix":matrix,"ttlMs":60_000,"code":code["code"]});
                Ok(state)
            }
            "companion_pair_cancel"
            | "companion_feedback"
            | "companion_approve"
            | "companion_update"
            | "companion_revoke"
            | "companion_prune"
            | "companion_unassign"
            | "companion_assign"
            | "companion_disconnect"
            | "companion_takeover" => {
                if command=="companion_assign" {
                    let state=self.status(services)?;
                    if let Some(client)=state["host"]["clients"].as_array().and_then(|clients|clients.iter().find(|client|client["companionId"]==payload["companionId"])){
                        validate_target_panel(&services.hub,payload["publicationHash"].as_str().unwrap_or(""),client,&state["firmware"][payload["companionId"].as_str().unwrap_or("")])?;
                    }
                }
                let mut request = payload;
                request["command"] = json!(command);
                let _cache=services.panel_cache.lock().unwrap_or_else(|p|p.into_inner());
                services.hub.command(&request)?;
                self.status(services)
            }
            _ => Err("Unsupported companion operator command.".into()),
        }
    }

}
