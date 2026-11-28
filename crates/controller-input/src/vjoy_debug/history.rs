use super::effects::{decode, Effects};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;

const MAX_ROWS: usize = 512;
const MAX_BYTES: usize = 180_000;
const MAX_PACKET: usize = 248;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    sequence: String,
    command: u32,
    wire_hex: String,
}

fn parse_cursor(text: &str) -> Result<u64, String> {
    text.parse::<u64>()
        .ok()
        .filter(|n| n.to_string() == text)
        .ok_or("Expected a canonical unsigned decimal cursor".into())
}
fn paging(args: &Value) -> Result<(u64, usize), String> {
    let after = match args.get("after") {
        Some(v) => parse_cursor(v.as_str().ok_or("Invalid after cursor")?)?,
        None => 0,
    };
    let limit = match args.get("limit") {
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=128).contains(n))
            .ok_or("Invalid page size")? as usize,
        None => 128,
    };
    Ok((after, limit))
}
#[derive(Default)]
pub(super) struct History {
    latest: u64,
    rows: VecDeque<Value>,
    effects: Effects,
    archive: Option<Vec<Row>>,
    recording_after: u64,
    bytes: usize,
    reason: Option<String>,
}
impl History {
    pub(super) fn observe(&mut self, slot: u8, cmd: u32, bytes: &[u8]) {
        let event = decode(slot, cmd, bytes);
        self.effects.observe(&event);
        self.latest += 1;
        let row = Row {
            sequence: self.latest.to_string(),
            command: cmd,
            wire_hex: bytes.iter().map(|b| format!("{b:02x}")).collect(),
        };
        if self.reason.is_none() {
            if let Some(rows) = &mut self.archive {
                let size = serde_json::to_vec(&row).unwrap().len() + 1;
                if rows.len() >= MAX_ROWS || self.bytes + size > MAX_BYTES {
                    self.reason = Some("capacity".into());
                } else {
                    self.bytes += size;
                    rows.push(row.clone());
                }
            }
        }
        let mut value = json!(row);
        value["event"] = event;
        if self.rows.len() == MAX_ROWS {
            self.rows.pop_front();
        }
        self.rows.push_back(value);
    }
    pub(super) fn record(&mut self) -> Result<(), String> {
        if self.archive.is_some() && self.reason.is_none() {
            return Err("Recording already active".into());
        }
        self.archive = Some(Vec::new());
        self.recording_after = self.latest;
        self.bytes = 0;
        self.reason = None;
        Ok(())
    }
    pub(super) fn stop(&mut self, reason: &str) -> Result<(), String> {
        if self.archive.is_none() {
            return Err("No vJoy recording was started".into());
        }
        self.reason.get_or_insert_with(|| reason.into());
        Ok(())
    }
    pub(super) fn status(&self) -> Value {
        json!({"status":if self.archive.is_none(){"idle"}else if self.reason.is_none(){"recording"}else{"stopped"},
        "records":self.archive.as_ref().map_or(0,Vec::len),"bytes":self.bytes,"reason":self.reason})
    }
    pub(super) fn export(&self, slot: u8, caps: &Value) -> Result<Value, String> {
        if self.reason.is_none() {
            return Err("Stop recording before exporting the observed prefix".into());
        }
        Ok(
            json!({"format":"osg-vjoy-ffb","slot":slot,"records":self.archive,"startingSequence":self.recording_after.to_string(),"capabilities":caps,"reason":self.reason}),
        )
    }
    pub(super) fn read(&self, args: &Value) -> Result<Value, String> {
        let (after, limit) = paging(args)?;
        let oldest = self
            .rows
            .front()
            .and_then(|r| r["sequence"].as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0u64);
        let mut result = page(&self.rows, oldest, self.latest, after, limit)?;
        result["effects"] = self.effects.snapshot();
        result["effectsThrough"] = json!(self.latest.to_string());
        result["recording"] = self.status();
        Ok(result)
    }
}

fn page(
    rows: &VecDeque<Value>,
    oldest: u64,
    latest: u64,
    after: u64,
    limit: usize,
) -> Result<Value, String> {
    if after > latest {
        return Err("Cursor exceeds the latest observed sequence".into());
    }
    let skipped = after
        .checked_sub(oldest)
        .map_or(0, |count| count.saturating_add(1))
        .min(rows.len() as u64) as usize;
    let entries: Vec<_> = rows.iter().skip(skipped).take(limit).collect();
    let next = entries.last().map_or_else(
        || after.to_string(),
        |row| row["sequence"].as_str().unwrap().to_owned(),
    );
    Ok(
        json!({"oldest":oldest.to_string(),"latest":latest.to_string(),"next":next,"gap":oldest>after.saturating_add(1),"hasMore":skipped+entries.len()<rows.len(),"records":entries}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    format: String,
    slot: u8,
    records: Vec<Row>,
    #[serde(rename = "startingSequence")]
    starting_sequence: Option<String>,
    capabilities: Option<Value>,
    reason: Option<String>,
}

pub(super) fn replay(args: &Value) -> Result<Value, String> {
    let (after, limit) = paging(args)?;
    let value = &args["recording"];
    if value.to_string().len() > 384_000 {
        return Err("vJoy archive exceeds import limit".into());
    }
    let archive: Archive = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if archive.format != "osg-vjoy-ffb"
        || !(1..=16).contains(&archive.slot)
        || archive.records.len() > MAX_ROWS
        || archive
            .capabilities
            .as_ref()
            .is_some_and(|v| !v.is_object())
        || archive.reason.as_ref().is_some_and(|r| r.len() > 256)
    {
        return Err("Invalid vJoy recording header or row count".into());
    }
    let start = archive
        .starting_sequence
        .as_deref()
        .map(parse_cursor)
        .transpose()?;
    let mut bytes = 0;
    let mut latest = 0u64;
    let mut oldest = 0u64;
    let mut rows = VecDeque::new();
    let mut effects = Effects::default();
    let mut returned = 0;
    for row in archive.records {
        let seq = parse_cursor(&row.sequence)?;
        if seq == 0 || (latest != 0 && latest.checked_add(1) != Some(seq)) {
            return Err("Recording sequences must be positive and contiguous".into());
        }
        if oldest == 0 {
            if start.is_some_and(|initial| initial.checked_add(1) != Some(seq)) {
                return Err("Recording does not begin at its declared sequence boundary".into());
            }
            oldest = seq;
        }
        latest = seq;
        bytes += serde_json::to_vec(&row).unwrap().len() + 1;
        if bytes > MAX_BYTES
            || row.wire_hex.len() > MAX_PACKET * 2
            || !row.wire_hex.len().is_multiple_of(2)
            || !row.wire_hex.is_ascii()
        {
            return Err("Invalid vJoy callback payload length".into());
        }
        let raw = row
            .wire_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Invalid hexadecimal callback payload")?;
        if !row.wire_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid hexadecimal callback payload".into());
        }
        let event = decode(archive.slot, row.command, &raw);
        if seq <= after || returned < limit {
            effects.observe(&event);
            if seq > after {
                returned += 1;
            }
        }
        let mut value = json!(row);
        value["event"] = event;
        rows.push_back(value);
    }
    let mut result = page(&rows, oldest, latest, after, limit)?;
    result["effects"] = effects.snapshot();
    result["priorCallbacksExcluded"] = json!(start.unwrap_or_else(|| oldest.saturating_sub(1)) > 0);
    result["effectsThrough"] = result["next"].clone();
    result["status"] = json!("replay");
    result["slot"] = json!(archive.slot);
    result["capabilities"] = json!(archive.capabilities);
    Ok(result)
}
