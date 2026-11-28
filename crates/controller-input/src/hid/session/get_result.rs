use super::*;
const MAX_RESULT_BYTES: usize = 2 * 1024 * 1024;
pub(super) struct Retained {
    id: String,
    json: Box<str>,
}
impl History {
    pub(super) fn get_bytes(&self) -> usize {
        self.last_get.as_ref().map_or(0, |result| result.json.len())
    }
    pub(super) fn retain_get(
        &mut self,
        id: &str,
        token: &str,
        mut value: Value,
        bounded: bool,
    ) -> Value {
        let encoded = value.to_string();
        let current = self.get_token.as_deref() == Some(token);
        let retained = current && encoded.len() <= MAX_RESULT_BYTES;
        if retained {
            self.last_get = Some(Retained {
                id: id.into(),
                json: encoded.clone().into_boxed_str(),
            });
            while self.bytes + self.get_bytes() > 8 * 1024 * 1024 {
                self.bytes -= self.entries.pop_front().unwrap().json.len();
            }
        }
        if bounded && encoded.len() > 900 * 1024 {
            value["samples"] = json!({"unavailable":if retained {
                "Decoded GET result exceeds response budget; read GET slice for exact JSON"
            } else {
                "Exact decoded GET result is unavailable from native retention; original bytes and platform status remain available"
            }});
            value["getSlice"] = if retained {
                json!({"requestId":id,"totalBytes":encoded.len(),"encoding":"json-utf8"})
            } else {
                json!({"requestId":id,"unavailable":if current {"capacity"} else {"stale-request"},
                    "maxBytes":MAX_RESULT_BYTES,"totalBytes":encoded.len(),
                    "reason":"Exact decoded GET result is not retained; this does not authorize another device read"})
            };
            if value.to_string().len() > 900 * 1024 {
                // Known backends keep wire/status outside samples. Do not let
                // an unexpected oversized property bypass the transport cap.
                value = json!({"wireHex":value["wireHex"].as_str().filter(|text|text.len()<=131072),
                    "kind":value["kind"].as_str().filter(|text|text.len()<=128),
                    "status":value.get("status").filter(|status|status.to_string().len()<=4096),
                    "error":{"kind":"capacity","message":"GET metadata exceeds response budget"},
                    "getSlice":value["getSlice"]});
            }
        }
        value
    }
    pub(super) fn get_slice(&self, request: &Value) -> Result<Value, String> {
        let id = super::super::required(request, "requestId")?;
        let result = self
            .last_get
            .as_ref()
            .filter(|result| result.id == id)
            .ok_or("Stale HID GET request or result exceeds native retention capacity")?;
        let mut slice = super::super::inspection::slice(result.json.as_bytes(), request)?;
        slice["requestId"] = json!(id);
        Ok(slice)
    }
}
