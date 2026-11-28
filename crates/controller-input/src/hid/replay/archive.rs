use super::super::values;
use super::{bounds, transfer};
use serde_json::{json, Value};
#[path = "raw.rs"]
mod raw;

pub(super) struct Entry {
    pub json: Box<str>,
    pub at: u64,
    pub offset: usize,
}
pub(super) struct Archive {
    pub owner: String,
    pub metadata: Value,
    pub descriptor: Value,
    pub entries: Vec<Entry>,
    pub bytes: usize,
    pub duration: u64,
    prefix: String,
    start: u64,
    initial_sequence: u64,
    current: values::Current,
    state_cursor: usize,
    latest: u64,
    terminal: Option<Value>,
}
impl Archive {
    pub fn parse(owner: String, input: &Value) -> Result<Self, String> {
        let encoded;
        let text = if let Some(text) = input.as_str() {
            text
        } else {
            bounds::archive_value(input)?;
            encoded = input.to_string();
            &encoded
        };
        bounds::text(text)?;
        let mut fields: raw::Fields =
            serde_json::from_str(text).map_err(|e| format!("Invalid HID recording JSON: {e}"))?;
        if let Some(recording) = fields.0.get("recording") {
            fields = serde_json::from_str(recording.get()).map_err(|e| e.to_string())?;
        } else if let Some(result) = fields.0.get("result") {
            let result: raw::Fields =
                serde_json::from_str(result.get()).map_err(|e| e.to_string())?;
            fields =
                serde_json::from_str(result.0.get("recording").ok_or("Missing recording")?.get())
                    .map_err(|e| e.to_string())?;
        }
        let rows: raw::Rows = serde_json::from_str(
            fields
                .0
                .remove("records")
                .ok_or("Missing recording records")?
                .get(),
        )
        .map_err(|e| e.to_string())?;
        let mut header = serde_json::Map::new();
        let mut header_bytes = 0usize;
        for (key, value) in fields.0 {
            let limit = if key == "descriptor" {
                bounds::DESCRIPTOR_BYTES
            } else {
                64 * 1024
            };
            if value.get().len() > limit {
                return Err("Recording metadata exceeds native byte bounds".into());
            }
            if key != "descriptor" {
                header_bytes += key.len() + value.get().len();
            }
            if header_bytes > 64 * 1024 {
                return Err("Recording metadata exceeds 64 KiB".into());
            }
            let value: Value = serde_json::from_str(value.get()).map_err(|e| e.to_string())?;
            bounds::value(&value)?;
            header.insert(key, value);
        }
        let input = &Value::Object(header);
        if input["format"] != "osg-hid-recording" {
            return Err("Expected osg-hid-recording archive".into());
        }
        for key in ["interfaceId", "generation"] {
            if input[key]
                .as_str()
                .is_none_or(|s| s.is_empty() || s.len() > 128)
            {
                return Err(format!("Invalid recording {key}"));
            }
        }
        if !input["descriptor"].is_object()
            || input["descriptor"].to_string().len() > bounds::DESCRIPTOR_BYTES
        {
            return Err("Invalid or oversized recording descriptor".into());
        }
        if !matches!(input["status"].as_str(), Some("recording" | "stopped"))
            || input.get("reason").is_none()
            || !(input["reason"].is_null() || input["reason"].is_object())
        {
            return Err("Invalid recording status or reason".into());
        }
        let start = bounds::decimal(&input["startedReceivedUs"], "startedReceivedUs")?;
        bounds::decimal(&input["startedUnixMs"], "startedUnixMs")?;
        let initial_sequence = bounds::decimal(&input["startingSequence"], "startingSequence")?;
        let last = bounds::decimal(&input["lastSequence"], "lastSequence")?;
        if last < initial_sequence {
            return Err("Recording last sequence precedes its start".into());
        }
        let count = bounds::integer(&input["recordCount"], "recordCount")?;
        let declared_bytes = bounds::integer(&input["bytes"], "bytes")?;
        if count != rows.0.len() {
            return Err("Recording recordCount conflicts with records".into());
        }
        let (mut time, mut sequence, mut ended) = (start, initial_sequence, false);
        let mut entries = Vec::with_capacity(rows.0.len());
        let mut row_position = 0usize;
        for raw_row in rows.0 {
            if raw_row.get().len() > 2 * 1024 * 1024 {
                return Err("Recording row exceeds 2 MiB".into());
            }
            let row: Value = serde_json::from_str(raw_row.get()).map_err(|e| e.to_string())?;
            bounds::value(&row)?;
            if ended || !row.is_object() {
                return Err("Invalid records after recording terminal".into());
            }
            let at = bounds::decimal(&row["receivedUs"], "receivedUs")?;
            if at < time {
                return Err("Recording timestamps must be ordered".into());
            }
            time = at;
            match row["kind"].as_str() {
                Some("gap") => {
                    if row.get("toSequence").is_none() {
                        return Err("Recording gap must declare its extent or null".into());
                    }
                    let from = bounds::decimal(&row["fromSequence"], "fromSequence")?;
                    if from <= sequence || !row["reason"].is_object() {
                        return Err("Invalid recording gap".into());
                    }
                    if from
                        != sequence
                            .checked_add(1)
                            .ok_or("Recording sequence overflow")?
                    {
                        return Err("Recording gap does not follow retained sequence".into());
                    }
                    if !row["toSequence"].is_null() {
                        let to = bounds::decimal(&row["toSequence"], "toSequence")?;
                        if to < from || to > last {
                            return Err("Invalid recording gap extent".into());
                        }
                        sequence = to;
                    }
                }
                Some("terminal") => {
                    let seq = bounds::decimal(&row["sequence"], "sequence")?;
                    if seq < sequence || seq > last || !row["reason"].is_object() {
                        return Err("Invalid recording terminal".into());
                    }
                    ended = true;
                }
                None => {
                    if row.get("kind").is_some() {
                        return Err("Invalid HID recording record kind".into());
                    }
                    let seq = bounds::decimal(&row["sequence"], "sequence")?;
                    if seq
                        != sequence
                            .checked_add(1)
                            .ok_or("Recording sequence overflow")?
                        || seq > last
                        || !row["samples"].is_object()
                    {
                        return Err("Recording report sequence or samples invalid".into());
                    }
                    sequence = seq;
                    if let Some(hex) = row.get("wireHex") {
                        let hex = hex.as_str().ok_or("Invalid recording wireHex")?;
                        if hex.len() > 131072
                            || hex.len() % 2 != 0
                            || !hex.bytes().all(|b| b.is_ascii_hexdigit())
                        {
                            return Err("Invalid recording wireHex".into());
                        }
                    }
                }
                _ => return Err("Unknown HID recording record kind".into()),
            }
            let encoded = row.to_string();
            if encoded.len() > 2 * 1024 * 1024 {
                return Err("Recording row exceeds 2 MiB".into());
            }
            entries.push(Entry {
                json: encoded.into_boxed_str(),
                at: at - start,
                offset: row_position,
            });
            row_position += entries.last().unwrap().json.len() + 1;
        }
        let mut metadata = Value::Object(
            input
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| key.as_str() != "records")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        let mut prefix = metadata.to_string();
        prefix.pop();
        prefix.push_str(",\"records\":[");
        let bytes = prefix.len()
            + 2
            + entries.iter().map(|r| r.json.len()).sum::<usize>()
            + entries.len().saturating_sub(1);
        if declared_bytes != bytes {
            return Err("Recording bytes conflicts with encoded archive".into());
        }
        if (ended
            && entries.last().is_some_and(|r| {
                serde_json::from_str::<Value>(&r.json).unwrap()["sequence"] != last.to_string()
            }))
            || (!ended && sequence != last)
        {
            return Err("Recording lastSequence conflicts with records".into());
        }
        if (input["status"] == "stopped") != ended
            || (ended
                && entries.last().is_some_and(|r| {
                    serde_json::from_str::<Value>(&r.json).unwrap()["reason"] != input["reason"]
                }))
        {
            return Err("Recording status or reason conflicts with terminal".into());
        }
        let descriptor = metadata
            .as_object_mut()
            .unwrap()
            .remove("descriptor")
            .unwrap();
        if metadata.to_string().len() > 64 * 1024 {
            return Err("Recording metadata exceeds 64 KiB".into());
        }
        metadata["entryCount"] = json!(entries.len());
        metadata["durationUs"] = json!((time - start).to_string());
        let current = values::Current::new(descriptor.clone());
        Ok(Self {
            owner,
            metadata,
            descriptor,
            entries,
            bytes,
            duration: time - start,
            prefix,
            start,
            initial_sequence,
            current,
            state_cursor: 0,
            latest: initial_sequence,
            terminal: None,
        })
    }
    pub fn export(&self, req: &Value) -> Result<Value, String> {
        let offset = req
            .get("byteOffset")
            .map(|n| bounds::integer(n, "byteOffset"))
            .transpose()?
            .unwrap_or(0);
        let limit = req
            .get("maxBytes")
            .map(|n| bounds::integer(n, "maxBytes"))
            .transpose()?
            .unwrap_or(bounds::CHUNK_BYTES);
        if offset > self.bytes || !(1..=bounds::CHUNK_BYTES).contains(&limit) {
            return Err("Invalid recording byte offset or maxBytes".into());
        }
        let end = offset.saturating_add(limit).min(self.bytes);
        let mut output = Vec::with_capacity(end - offset);
        let mut copy = |position: usize, part: &[u8]| {
            let part_end = position + part.len();
            if position < end && part_end > offset {
                output.extend_from_slice(
                    &part[offset.saturating_sub(position)..(end - position).min(part.len())],
                );
            }
        };
        copy(0, self.prefix.as_bytes());
        let first = self
            .entries
            .partition_point(|r| self.prefix.len() + r.offset + r.json.len() < offset);
        for (index, row) in self.entries.iter().enumerate().skip(first) {
            let position = self.prefix.len() + row.offset;
            if index > 0 {
                copy(position - 1, b",");
            }
            if position >= end {
                break;
            }
            copy(position, row.json.as_bytes());
        }
        copy(self.bytes - 2, b"]}");
        Ok(
            json!({"chunk":transfer::encode(&output),"encoding":"base64","nextByteOffset":end,"totalBytes":self.bytes,"complete":end==self.bytes}),
        )
    }
    pub fn page(&mut self, req: &Value) -> Result<Value, String> {
        let after = req
            .get("after")
            .map(|n| bounds::decimal(n, "after"))
            .transpose()?
            .unwrap_or(0);
        let mut after = usize::try_from(after).map_err(|_| "Invalid replay cursor")?;
        if after > self.entries.len() {
            return Err("Replay cursor is ahead of archive".into());
        }
        let limit = req
            .get("limit")
            .map(|n| bounds::integer(n, "limit"))
            .transpose()?
            .unwrap_or(128);
        if !(1..=128).contains(&limit) {
            return Err("Replay limit must be 1..128".into());
        }
        let clock = req
            .get("elapsedUs")
            .map(|n| {
                n.as_u64()
                    .map(Ok)
                    .unwrap_or_else(|| bounds::decimal(n, "elapsedUs"))
            })
            .transpose()?
            .unwrap_or(u64::MAX);
        if let Some(seek) = req.get("seek") {
            let seek = seek.as_bool().ok_or("Replay seek must be boolean")?;
            if seek {
                if after != 0 || req.get("elapsedUs").is_none() {
                    return Err("Replay seek requires after 0 and elapsedUs".into());
                }
                let end = self.entries.partition_point(|r| r.at <= clock);
                after = end;
                let mut tail_bytes = 0;
                for row in self.entries[..end].iter().rev().take(limit) {
                    if after < end && tail_bytes + row.json.len() > 900 * 1024 {
                        break;
                    }
                    tail_bytes += row.json.len();
                    after -= 1;
                }
            }
        }
        if after > 0 && self.entries[after - 1].at > clock {
            return Err("Replay cursor is beyond requested elapsedUs; seek from after 0".into());
        }
        let mut next = after;
        let mut bytes = 0;
        let mut records = Vec::new();
        for row in self
            .entries
            .iter()
            .skip(after)
            .take(limit)
            .take_while(|r| r.at <= clock)
        {
            if !records.is_empty() && bytes + row.json.len() > 900 * 1024 {
                break;
            }
            records
                .push(serde_json::from_str::<Value>(&row.json).expect("validated recording row"));
            bytes += row.json.len();
            next += 1;
        }
        if next < self.state_cursor {
            self.current = values::Current::new(self.descriptor.clone());
            self.state_cursor = 0;
            self.latest = self.initial_sequence;
            self.terminal = None;
        }
        for entry in self.entries.iter().take(next).skip(self.state_cursor) {
            let row: Value = serde_json::from_str(&entry.json).expect("validated recording row");
            match row["kind"].as_str() {
                Some("gap") => {
                    self.current.clear();
                    if !row["toSequence"].is_null() {
                        self.latest = bounds::decimal(&row["toSequence"], "toSequence")?;
                    }
                }
                Some("terminal") => {
                    self.current.clear();
                    self.terminal = Some(row["reason"].clone());
                    self.latest = bounds::decimal(&row["sequence"], "sequence")?;
                }
                _ => {
                    self.latest = bounds::decimal(&row["sequence"], "sequence")?;
                    self.current.observe(
                        self.latest,
                        u128::from(self.start + entry.at),
                        &row["samples"],
                        row.get("provenance"),
                    );
                }
            }
        }
        self.state_cursor = next;
        Ok(
            json!({"records":records,"next":next.to_string(),"hasMore":self.entries.get(next).is_some_and(|r|r.at<=clock),
            "complete":next==self.entries.len(),"positionUs":if clock==u64::MAX {self.entries.get(next.wrapping_sub(1)).map_or(0,|r|r.at)} else {clock.min(self.duration)}.to_string(),
            "durationUs":self.duration.to_string(),"state":self.current.state(self.latest,self.terminal.as_ref())}),
        )
    }
}
