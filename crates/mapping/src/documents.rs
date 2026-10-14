//! Bounded portable document transfer. Uploading never constructs a mapping runtime.
use crate::{
    profile::{Profile, DOCUMENT_LIMIT},
    store::Store,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
pub const CHUNK_LIMIT: usize = 8192;
pub const UPLOAD_LIMIT: usize = 4;
const TTL: Duration = Duration::from_secs(30);
struct Upload {
    owner: String,
    size: usize,
    bytes: Vec<u8>,
    started: Instant,
}
pub struct Documents {
    store: Option<Store>,
    uploads: Mutex<BTreeMap<String, Upload>>,
}
fn integer(request: &Value, key: &str) -> Result<usize, String> {
    request[key]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("{key} must be a nonnegative integer."))
}
fn text<'a>(request: &'a Value, key: &str) -> Result<&'a str, String> {
    request[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 80)
        .ok_or_else(|| format!("{key} must be a nonempty string of at most 80 bytes."))
}
impl Documents {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            store: root.map(Store::new),
            uploads: Mutex::new(BTreeMap::new()),
        }
    }
    pub fn clear(&self) {
        self.uploads
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
    pub fn request(&self, request: &Value) -> Result<Value, String> {
        self.request_at(request, Instant::now())
    }
    fn request_at(&self, request: &Value, now: Instant) -> Result<Value, String> {
        if !request.is_object() || request.to_string().len() > DOCUMENT_LIMIT {
            return Err("Invalid mapping document request.".into());
        }
        let owner = text(request, "clientId")?;
        let command = text(request, "command")?;
        let mut uploads = self.uploads.lock().unwrap_or_else(|p| p.into_inner());
        uploads.retain(|_, u| now.saturating_duration_since(u.started) < TTL);
        match command {
            "mapping_import_begin" => {
                let size = integer(request, "size")?;
                if size == 0 || size > DOCUMENT_LIMIT {
                    return Err("Document must contain 1 to 65536 UTF-8 bytes.".into());
                }
                if uploads.len() >= UPLOAD_LIMIT || uploads.values().any(|u| u.owner == owner) {
                    return Err(
                        "An upload is already active; finish or abort it, or wait 30 seconds."
                            .into(),
                    );
                }
                let id = uuid::Uuid::new_v4().to_string();
                uploads.insert(
                    id.clone(),
                    Upload {
                        owner: owner.into(),
                        size,
                        bytes: Vec::with_capacity(size),
                        started: now,
                    },
                );
                Ok(json!({"schema":1,"uploadId":id,"offset":0,"chunkLimit":CHUNK_LIMIT}))
            }
            "mapping_import_chunk"
            | "mapping_import_commit"
            | "mapping_import_abort"
            | "mapping_validate_commit" => {
                let id = text(request, "uploadId")?;
                let upload = uploads
                    .get_mut(id)
                    .filter(|u| u.owner == owner)
                    .ok_or("Upload is unknown, expired, or belongs to another client.")?;
                if command == "mapping_import_chunk" {
                    let offset = integer(request, "offset")?;
                    let bytes = request["bytes"]
                        .as_array()
                        .ok_or("Chunk requires a byte array.")?;
                    if bytes.is_empty()
                        || bytes.len() > CHUNK_LIMIT
                        || offset != upload.bytes.len()
                        || bytes.len() > upload.size - upload.bytes.len()
                    {
                        return Err("Chunk size or ordered offset is invalid.".into());
                    }
                    // Validate the complete chunk before mutating the upload.
                    let bytes: Vec<u8> = bytes
                        .iter()
                        .map(|b| {
                            b.as_u64()
                                .and_then(|v| u8::try_from(v).ok())
                                .ok_or_else(|| {
                                    "Chunks contain only integer bytes from 0 to 255.".to_string()
                                })
                        })
                        .collect::<Result<_, _>>()?;
                    upload.bytes.extend_from_slice(&bytes);
                    return Ok(json!({"schema":1,"uploadId":id,"offset":upload.bytes.len()}));
                }
                if command == "mapping_import_abort" {
                    uploads.remove(id);
                    return Ok(json!({"schema":1,"aborted":true}));
                }
                if upload.bytes.len() != upload.size {
                    return Err("Upload is incomplete.".into());
                }
                let bytes = uploads.remove(id).unwrap().bytes;
                let profile = Profile::parse(&bytes)?;
                if command == "mapping_import_commit" {
                    self.store
                        .as_ref()
                        .ok_or("Mapping storage is unavailable.")?
                        .save(&profile)?;
                }
                Ok(
                    json!({"schema":1,"document":profile,"saved":command == "mapping_import_commit"}),
                )
            }
            "mapping_profiles" => {
                let offset = integer(request, "offset")?;
                let ids = self
                    .store
                    .as_ref()
                    .ok_or("Mapping storage is unavailable.")?
                    .ids()?;
                if offset > ids.len() {
                    return Err("Invalid profile page offset.".into());
                }
                let page: Vec<_> = ids.iter().skip(offset).take(32).collect();
                let next = offset + page.len();
                Ok(
                    json!({"schema":1,"ids":page,"next":if next < ids.len() { Some(next) } else { None }}),
                )
            }
            "mapping_export" => {
                let id = text(request, "profileId")?;
                let document = self
                    .store
                    .as_ref()
                    .ok_or("Mapping storage is unavailable.")?
                    .load(id)?;
                Ok(json!({"schema":1,"document":document}))
            }
            _ => Err("Unknown mapping document command.".into()),
        }
    }
}
#[cfg(test)]
#[path = "documents_tests.rs"]
mod tests;
