//! Local operator controls. The WSS endpoint never exposes these commands.
use osg_companion_service::server::{ServerConfig, ServerRuntime};
use osg_companion_service::{qrcode_matrix, Hub};
use osg_mapping_host::companions::CompanionProvider;
use serde_json::{json, Value};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

const MAX_UPLOAD: usize = 13 * 1024 * 1024;
struct Upload {
    id: String,
    owner: String,
    expected: usize,
    bytes: Vec<u8>,
    started: Instant,
}
struct Services {
    hub: Hub,
    server: ServerRuntime,
}
pub struct Runtime {
    path: Option<PathBuf>,
    provider: Arc<CompanionProvider>,
    services: OnceLock<Result<Services, String>>,
    upload: Mutex<Option<Upload>>,
}
impl Runtime {
    pub fn new(path: Option<PathBuf>, provider: Arc<CompanionProvider>) -> Self {
        let runtime = Self {
            path,
            provider,
            services: OnceLock::new(),
            upload: Mutex::new(None),
        };
        // Restore retained offline source descriptors while leaving network startup disabled.
        if runtime.path.is_some() {
            let _ = runtime.services();
        }
        runtime
    }
    fn services(&self) -> Result<&Services, String> {
        self.services
            .get_or_init(|| {
                let (hub, directory) = if let Some(path) = &self.path {
                    (
                        Hub::open(path.clone(), self.provider.clone())?,
                        path.clone(),
                    )
                } else {
                    (
                        Hub::new(
                            format!("host-{}", uuid::Uuid::new_v4()),
                            self.provider.clone(),
                        ),
                        std::env::temp_dir()
                            .join(format!("osg-companion-test-{}", uuid::Uuid::new_v4())),
                    )
                };
                let server = ServerRuntime::new(hub.clone(), directory)?;
                Ok(Services { hub, server })
            })
            .as_ref()
            .map_err(Clone::clone)
    }
    fn status(&self, services: &Services) -> Result<Value, String> {
        let interfaces: Vec<String> = osg_companion_service::server::local_interfaces()
            .ok()
            .and_then(|value| {
                value.as_array().map(|rows| {
                    rows.iter()
                        .filter_map(|row| row["ip"].as_str().map(str::to_owned))
                        .collect()
                })
            })
            .unwrap_or_default();
        Ok(
            json!({"host":services.hub.command(&json!({"command":"companion_status"}))?, "listener":services.server.status(), "interfaces":interfaces}),
        )
    }
    pub fn request(&self, request: &Value) -> Result<Value, String> {
        let command = request["command"]
            .as_str()
            .ok_or("Missing companion command.")?;
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
        match command {
            "companion_publish_begin" => {
                let expected = payload["size"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= MAX_UPLOAD as u64)
                    .ok_or("Panel upload must contain 1–13 MiB.")?
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
                        .ok_or("Panel chunks contain 1–8192 bytes.")?;
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
                if object.len() != 3
                    || !["canonicalDocument", "publicationHash", "assets"]
                        .iter()
                        .all(|k| object.contains_key(*k))
                {
                    return Err("Unexpected panel upload fields.".into());
                }
                object.insert("command".into(), json!("companion_publish"));
                let services = self.services()?;
                services.hub.command(&value)?;
                return self.status(services);
            }
            _ => {}
        }
        let services = self.services()?;
        match command {
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
                services.server.start(ServerConfig {
                    bind: SocketAddr::new(address, port),
                    advertised_host,
                    discovery: payload["discovery"] == true,
                })?;
                self.status(services)
            }
            "companion_disable" => {
                services.server.stop();
                self.status(services)
            }
            "companion_pair_offer" => {
                let offer = services
                    .hub
                    .command(&json!({"command":"companion_pair_offer"}))?;
                let secret = offer["secret"]
                    .as_str()
                    .ok_or("Pairing offer did not contain a secret.")?;
                let expires = offer["expiresAtMs"]
                    .as_u64()
                    .ok_or("Pairing offer did not contain an expiry.")?;
                let payload = services.server.pairing_payload(secret, expires)?;
                let manual = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
                let matrix = qrcode_matrix(&manual)?;
                let mut state = self.status(services)?;
                state["pairing"] = json!({"manualPayload":manual,"qrMatrix":matrix,"ttlMs":60_000});
                Ok(state)
            }
            "companion_feedback"
            | "companion_approve"
            | "companion_revoke"
            | "companion_assign"
            | "companion_disconnect"
            | "companion_takeover" => {
                let mut request = payload;
                request["command"] = json!(command);
                services.hub.command(&request)?;
                self.status(services)
            }
            _ => Err("Unsupported companion operator command.".into()),
        }
    }
    pub fn shutdown(&self) {
        if let Some(Ok(services)) = self.services.get() {
            services.server.stop();
        }
        self.upload.lock().unwrap_or_else(|p| p.into_inner()).take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime() -> Runtime {
        Runtime::new(None, Arc::new(CompanionProvider::new(Arc::new(|| 1))))
    }
    fn call(runtime: &Runtime, command: &str, payload: Value) -> Result<Value, String> {
        runtime.request(&json!({"command":command,"clientId":"operator-one","companion":payload}))
    }
    #[test]
    fn uploads_are_bounded_ordered_and_owned_without_initializing_the_listener() {
        let runtime = runtime();
        assert!(call(&runtime, "companion_publish_begin", json!({"size":0})).is_err());
        assert!(call(
            &runtime,
            "companion_publish_begin",
            json!({"size":MAX_UPLOAD+1})
        )
        .is_err());
        let id = call(&runtime, "companion_publish_begin", json!({"size":3})).unwrap()["uploadId"]
            .clone();
        assert!(call(&runtime, "companion_publish_begin", json!({"size":2})).is_err());
        assert!(runtime.request(&json!({"command":"companion_publish_chunk","clientId":"operator-two","companion":{"uploadId":id,"offset":0,"bytes":[1]}})).is_err());
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":1,"bytes":[1]})
        )
        .is_err());
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[999]})
        )
        .is_err());
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[1,2,3,4]})
        )
        .is_err());
        assert!(call(&runtime, "companion_publish_commit", json!({"uploadId":id})).is_err());
        call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[1,2,3]}),
        )
        .unwrap();
        assert!(call(&runtime, "companion_publish_commit", json!({"uploadId":id})).is_err());
        assert!(runtime.services.get().is_none());
        runtime.shutdown();
    }
    #[test]
    fn upload_abort_and_expiry_preserve_other_operator_ownership() {
        let runtime = runtime();
        let id = call(&runtime, "companion_publish_begin", json!({"size":1})).unwrap()["uploadId"]
            .clone();
        runtime.request(&json!({"command":"companion_publish_abort","clientId":"other","companion":{"uploadId":id}})).unwrap();
        assert!(call(&runtime, "companion_publish_begin", json!({"size":1})).is_err());
        call(&runtime, "companion_publish_abort", json!({"uploadId":id})).unwrap();
        let id = call(&runtime, "companion_publish_begin", json!({"size":1})).unwrap()["uploadId"]
            .clone();
        runtime.upload.lock().unwrap().as_mut().unwrap().started =
            Instant::now() - Duration::from_secs(61);
        assert!(call(
            &runtime,
            "companion_publish_chunk",
            json!({"uploadId":id,"offset":0,"bytes":[1]})
        )
        .unwrap_err()
        .contains("expired"));
        assert!(runtime.upload.lock().unwrap().is_none());
        assert!(runtime.request(&json!({})).is_err());
        assert!(runtime
            .request(&json!({"command":"companion_status","companion":false}))
            .is_err());
        assert!(runtime.services.get().is_none());
    }
    #[cfg(windows)]
    #[test]
    fn native_operator_status_pairing_publication_and_assignment_remain_separate_from_output() {
        let runtime = runtime();
        let initial = call(&runtime, "companion_status", json!({})).unwrap();
        assert_eq!(initial["listener"]["enabled"], false);
        assert!(initial["interfaces"].is_array());
        assert!(call(
            &runtime,
            "companion_enable",
            json!({"address":"invalid","port":54437,"advertisedHost":"127.0.0.1"})
        )
        .is_err());
        assert!(call(
            &runtime,
            "companion_enable",
            json!({"address":"127.0.0.1","port":1,"advertisedHost":"127.0.0.1"})
        )
        .is_err());
        assert!(call(&runtime, "companion_pair_offer", json!({})).is_err());
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = occupied.local_addr().unwrap().port();
        let config = json!({"address":"127.0.0.1","port":port,"advertisedHost":"127.0.0.1","discovery":false});
        assert!(call(&runtime, "companion_enable", config.clone())
            .unwrap_err()
            .contains("Cannot bind"));
        drop(occupied);
        assert_eq!(
            call(&runtime, "companion_enable", config.clone()).unwrap()["listener"]["enabled"],
            true
        );
        assert!(call(&runtime, "companion_enable", config).is_err());
        let offer = call(&runtime, "companion_pair_offer", json!({})).unwrap();
        assert!(offer["pairing"]["qrMatrix"].as_array().unwrap().len() > 20);
        let pairing: Value =
            serde_json::from_str(offer["pairing"]["manualPayload"].as_str().unwrap()).unwrap();
        assert!(pairing["endpoint"]
            .as_str()
            .unwrap()
            .starts_with("wss://127.0.0.1:"));
        let services = runtime.services().unwrap();
        let pending=services.hub.handle_wire(json!({"type":"pair","version":1,"secret":pairing["secret"],"companionId":"test-installation","name":"Desktop panel"}),"root-test-peer",services.hub.now_ms());
        assert_eq!(pending["type"], "pair-pending");
        call(
            &runtime,
            "companion_approve",
            json!({"requestId":pending["requestId"],"companionId":"test-installation"}),
        )
        .unwrap();

        let fixture: Value = serde_json::from_str(include_str!(
            "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
        ))
        .unwrap();
        let upload = json!({"canonicalDocument":fixture["canonicalDocument"],"publicationHash":fixture["publicationHash"],"assets":fixture["assets"]});
        let bytes = serde_json::to_vec(&upload).unwrap();
        let upload_id = call(
            &runtime,
            "companion_publish_begin",
            json!({"size":bytes.len()}),
        )
        .unwrap()["uploadId"]
            .clone();
        for (index, chunk) in bytes.chunks(8192).enumerate() {
            call(
                &runtime,
                "companion_publish_chunk",
                json!({"uploadId":upload_id,"offset":index*8192,"bytes":chunk}),
            )
            .unwrap();
        }
        let published = call(
            &runtime,
            "companion_publish_commit",
            json!({"uploadId":upload_id}),
        )
        .unwrap();
        assert_eq!(
            published["host"]["publications"].as_array().unwrap().len(),
            1
        );
        let assigned=call(&runtime,"companion_assign",json!({"companionId":"test-installation","instanceId":"saved-panel","publicationHash":fixture["publicationHash"]})).unwrap();
        assert_eq!(assigned["host"]["instances"][0]["armed"], false);
        assert!(call(
            &runtime,
            "companion_approve",
            json!({"requestId":"missing"})
        )
        .is_err());
        call(
            &runtime,
            "companion_disconnect",
            json!({"instanceId":"saved-panel"}),
        )
        .unwrap();
        call(
            &runtime,
            "companion_takeover",
            json!({"instanceId":"saved-panel","companionId":"test-installation"}),
        )
        .unwrap();
        call(
            &runtime,
            "companion_revoke",
            json!({"companionId":"test-installation"}),
        )
        .unwrap();
        assert!(call(&runtime, "companion_unsupported", json!({})).is_err());
        assert_eq!(
            call(&runtime, "companion_disable", json!({})).unwrap()["listener"]["enabled"],
            false
        );
        runtime.shutdown();
    }
}
