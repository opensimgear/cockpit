use super::*;
use std::borrow::Cow;

impl Recording {
    pub(super) fn make_parts(&self) -> (String, String) {
        let mut metadata = self.metadata.clone();
        metadata["recordCount"] = json!(self.records.len());
        metadata["bytes"] = json!(0);
        let mut bytes = 0;
        // The decimal byte count participates in its own encoded size.
        for _ in 0..8 {
            metadata["bytes"] = json!(bytes);
            let size =
                metadata.to_string().len() + self.row_bytes + self.records.len().saturating_sub(1);
            if size == bytes {
                break;
            }
            bytes = size;
        }
        metadata["bytes"] = json!(bytes);
        let text = metadata.to_string();
        let position = text
            .rfind("\"records\":[]")
            .expect("recording records placeholder");
        let insertion = position + "\"records\":[".len();
        (
            format!("{{\"recording\":{}", &text[..insertion]),
            format!("{}}}", &text[insertion..]),
        )
    }
    fn parts(&self) -> (Cow<'_, str>, Cow<'_, str>) {
        match &self.sealed {
            Some((head, tail)) => (Cow::Borrowed(head), Cow::Borrowed(tail)),
            None => {
                let (head, tail) = self.make_parts();
                (Cow::Owned(head), Cow::Owned(tail))
            }
        }
    }
    pub(super) fn encoded_size(&self) -> usize {
        if self.active() {
            // Status polling must not copy/encode the descriptor while capture
            // waits for this mutex. Only these decimal fields grow when live.
            let digits = |n: usize| if n == 0 { 1 } else { n.ilog10() as usize + 1 };
            let fixed = self.initial_metadata_bytes - self.initial_sequence_digits - 2
                + self.metadata["lastSequence"].as_str().unwrap().len()
                + digits(self.records.len())
                + self.row_bytes
                + self.records.len().saturating_sub(1);
            let mut bytes = fixed + 1;
            loop {
                let next = fixed + digits(bytes);
                if next == bytes {
                    return bytes;
                }
                bytes = next;
            }
        }
        let (head, tail) = self.parts();
        head.len() + self.row_bytes + self.records.len().saturating_sub(1) + tail.len()
            - "{\"recording\":}".len()
    }
    pub(super) fn export(&self, req: &Value) -> Result<Value, String> {
        if self.active() {
            return Err("Stop recording before export".into());
        }
        let (head, tail) = self.parts();
        let total = head.len() + self.row_bytes + self.records.len().saturating_sub(1) + tail.len();
        let chunked = req.get("offset").is_some() || req.get("limit").is_some();
        let offset = req.get("offset").map_or(Ok(0), |v| {
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("Invalid recording byte offset")
        })?;
        let limit = req.get("limit").map_or(Ok(32768), |v| {
            v.as_u64()
                .filter(|v| (1..=32768).contains(v))
                .map(|v| v as usize)
                .ok_or("Recording byte limit must be 1..32768")
        })?;
        if offset > total {
            return Err("Recording byte offset exceeds archive".into());
        }
        let end = if chunked {
            offset.saturating_add(limit).min(total)
        } else {
            total
        };
        let start = if chunked { offset } else { 0 };
        let mut output = Vec::with_capacity(end - start);
        let mut append = |position: usize, part: &[u8]| {
            let part_end = position + part.len();
            if part_end > start && position < end {
                output.extend_from_slice(
                    &part[start.saturating_sub(position)..(end - position).min(part.len())],
                );
            }
        };
        append(0, head.as_bytes());
        let first = self
            .positions
            .partition_point(|p| *p <= start.saturating_sub(head.len()))
            .saturating_sub(1);
        for (i, row) in self.records.iter().enumerate().skip(first) {
            let position = head.len() + self.positions[i];
            if i != 0 {
                append(position - 1, b",");
            }
            if position >= end {
                break;
            }
            append(position, row.as_bytes());
        }
        append(total - tail.len(), tail.as_bytes());
        if chunked {
            Ok(json!({"chunk":base64(&output),"offset":offset,"nextOffset":end,"totalBytes":total}))
        } else {
            serde_json::from_slice(&output).map_err(|e| format!("Recording export failed: {e}"))
        }
    }
}

pub(super) use super::super::transfer::encode as base64;
