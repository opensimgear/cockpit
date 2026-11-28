//! Bounded deterministic observation-only archives; no device or routing handles.
use super::transfer;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use uuid::Uuid;
#[path = "replay/archive.rs"]
mod archive;
#[path = "replay/bounds.rs"]
mod bounds;

#[derive(Default)]
pub(super) struct Store {
    replays: HashMap<String, archive::Archive>,
    uploads: HashMap<String, Upload>,
}
struct Upload {
    owner: String,
    total: usize,
    bytes: Vec<u8>,
    touched: Instant,
}
fn owner(req: &Value) -> Result<&str, String> {
    match req.get("clientId") {
        None => Ok("portal"),
        Some(v) => v
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or_else(|| "Invalid replay clientId".into()),
    }
}
fn id<'a>(req: &'a Value, key: &str) -> Result<&'a str, String> {
    req[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or_else(|| format!("Missing or invalid {key}"))
}
impl Store {
    fn occupied(&self) -> usize {
        self.replays.values().map(|r| r.bytes).sum::<usize>()
            + self.uploads.values().map(|u| u.total).sum::<usize>()
    }
    fn import(&mut self, req: &Value, summary: bool) -> Result<Value, String> {
        if self.replays.len() >= 4 {
            return Err("Replay capacity reached; close an imported replay".into());
        }
        let archive = archive::Archive::parse(
            owner(req)?.to_owned(),
            req.get("recording").ok_or("Missing recording")?,
        )?;
        if self.occupied().saturating_add(archive.bytes) > bounds::TOTAL_BYTES {
            return Err("Replay memory capacity reached; close an imported replay".into());
        }
        let replay_id = Uuid::new_v4().to_string();
        let mut response = json!({"replayId":replay_id,"metadata":archive.metadata});
        if !summary {
            response["descriptor"] = archive.descriptor.clone();
        }
        self.replays.insert(replay_id, archive);
        Ok(response)
    }
    pub fn request(&mut self, command: &str, req: &Value) -> Result<Value, String> {
        self.uploads
            .retain(|_, u| u.touched.elapsed() < Duration::from_secs(300));
        match command {
            "controller_hid_replay_import" => self.import(req, false),
            "controller_hid_replay_upload_start" => {
                let total = bounds::integer(&req["totalBytes"], "totalBytes")?;
                if total == 0 || total > bounds::ARCHIVE_BYTES {
                    return Err("Replay upload must be 1 byte..64 MiB".into());
                }
                if self.uploads.len() >= 4
                    || self.occupied().saturating_add(total) > bounds::TOTAL_BYTES
                {
                    return Err(
                        "Replay upload memory capacity reached; close or cancel existing work"
                            .into(),
                    );
                }
                let upload_id = Uuid::new_v4().to_string();
                self.uploads.insert(
                    upload_id.clone(),
                    Upload {
                        owner: owner(req)?.to_owned(),
                        total,
                        bytes: Vec::new(),
                        touched: Instant::now(),
                    },
                );
                Ok(
                    json!({"uploadId":upload_id,"totalBytes":total,"maxChunkBytes":bounds::CHUNK_BYTES,"expiresAfterSeconds":300}),
                )
            }
            "controller_hid_replay_upload_chunk" => {
                let upload = self
                    .uploads
                    .get_mut(id(req, "uploadId")?)
                    .filter(|u| u.owner == owner(req).unwrap_or(""))
                    .ok_or("Unknown replay upload for this client")?;
                let offset = bounds::integer(&req["offset"], "offset")?;
                let chunk = transfer::decode(
                    req["chunk"]
                        .as_str()
                        .ok_or("Expected base64 replay chunk")?,
                )?;
                if chunk.len() > bounds::CHUNK_BYTES {
                    return Err("Replay chunk exceeds advertised 24 KiB".into());
                }
                let end = offset
                    .checked_add(chunk.len())
                    .filter(|end| *end <= upload.total)
                    .ok_or("Replay upload exceeds declared bytes")?;
                if offset < upload.bytes.len() {
                    if upload.bytes.get(offset..end) != Some(chunk.as_slice()) {
                        return Err("Conflicting replay upload retry".into());
                    }
                } else {
                    if offset != upload.bytes.len() {
                        return Err("Replay upload offset must follow accepted bytes".into());
                    }
                    upload.bytes.extend_from_slice(&chunk);
                }
                upload.touched = Instant::now();
                Ok(
                    json!({"uploadId":req["uploadId"],"nextOffset":upload.bytes.len(),"complete":upload.bytes.len()==upload.total}),
                )
            }
            "controller_hid_replay_upload_finish" => {
                let key = id(req, "uploadId")?;
                let upload = self
                    .uploads
                    .get(key)
                    .filter(|u| u.owner == owner(req).unwrap_or(""))
                    .ok_or("Unknown replay upload for this client")?;
                if upload.bytes.len() != upload.total {
                    return Err("Replay upload is incomplete".into());
                }
                let upload = self.uploads.remove(key).unwrap();
                let text = String::from_utf8(upload.bytes)
                    .map_err(|_| "Replay upload must be UTF-8 JSON")?;
                self.import(&json!({"clientId":upload.owner,"recording":text}), true)
            }
            "controller_hid_replay_upload_cancel" => {
                let key = id(req, "uploadId")?;
                if self
                    .uploads
                    .get(key)
                    .is_none_or(|u| u.owner != owner(req).unwrap_or(""))
                {
                    return Err("Unknown replay upload for this client".into());
                }
                self.uploads.remove(key);
                Ok(json!({"cancelled":true}))
            }
            "controller_hid_replay_read" | "controller_hid_replay_close" => {
                let key = id(req, "replayId")?;
                let archive = self
                    .replays
                    .get_mut(key)
                    .filter(|a| a.owner == owner(req).unwrap_or(""))
                    .ok_or("Unknown offline replay for this client")?;
                if command == "controller_hid_replay_close" {
                    self.replays.remove(key);
                    return Ok(json!({"replayId":key,"closed":true}));
                }
                let mut page = match req["part"].as_str() {
                    Some("recording") => archive.export(req)?,
                    Some("descriptor") => json!({"descriptor":archive.descriptor}),
                    Some("state") => {
                        let mut exact = req.clone();
                        exact.as_object_mut().unwrap().remove("bounded");
                        json!({"state":archive.page(&exact)?["state"]})
                    }
                    None if req.get("part").is_none() => archive.page(req)?,
                    _ => return Err("Replay part must be recording, descriptor or state".into()),
                };
                page["replayId"] = json!(key);
                Ok(page)
            }
            _ => Err("Unknown HID replay command".into()),
        }
    }
    pub fn clear(&mut self) {
        self.replays.clear();
        self.uploads.clear();
    }
    pub(super) fn release_client(&mut self, client: &str) {
        self.replays.retain(|_, archive| archive.owner != client);
        self.uploads.retain(|_, upload| upload.owner != client);
    }
}
#[cfg(test)]
#[path = "replay/native_tests.rs"]
mod native_tests;
#[cfg(test)]
#[path = "replay/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "replay/validation_tests.rs"]
mod validation_tests;

#[cfg(test)]
#[path = "replay/bounded_tests.rs"]
mod bounded_tests;
