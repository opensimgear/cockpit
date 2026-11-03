use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

const CHUNK_BYTES: usize = 8192;

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub bytes: usize,
    pub blobs: usize,
    pub blob_bytes: usize,
    pub scopes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { bytes: 1 << 30, blobs: 65536, blob_bytes: 16 << 20, scopes: 66 }
    }
}

#[derive(Clone)]
struct Reference {
    id: String,
    kind: &'static str,
}

struct Blob {
    text: Arc<str>,
    dependencies: HashMap<&'static str, Vec<Reference>>,
    pins: usize,
    touched: u64,
}

#[derive(Default)]
struct State {
    blobs: HashMap<String, Blob>,
    scopes: HashMap<String, HashSet<String>>,
    provisional: HashSet<String>,
    bytes: usize,
    clock: u64,
}

pub(crate) struct Pages {
    writer: Mutex<()>,
    state: Mutex<State>,
    limits: Limits,
}

impl Default for Pages {
    fn default() -> Self { Self::with_limits(Limits::default()) }
}

impl Pages {
    pub(super) fn with_limits(limits: Limits) -> Self {
        Self { writer: Mutex::new(()), state: Mutex::new(State::default()), limits }
    }

    pub(crate) fn begin(&self) -> Projection<'_> {
        Projection {
            pages: self,
            _writer: self.writer.lock().unwrap_or_else(|e| e.into_inner()),
            references: HashSet::new(),
            created: HashSet::new(),
            committed: false,
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn get(&self, id: &str, offset: u64) -> Result<Value, String> {
        validate_id(id)?;
        let text = {
            let mut state = self.state();
            state.clock = state.clock.wrapping_add(1);
            let clock = state.clock;
            let blob = state.blobs.get_mut(id).ok_or("Game page has expired or was not issued")?;
            blob.touched = clock;
            Arc::clone(&blob.text)
        };
        let start = usize::try_from(offset).map_err(|_| "Invalid game page offset")?;
        if start > text.len() || !text.is_char_boundary(start) {
            return Err("Invalid game page offset".into());
        }
        let mut end = text.len().min(start.saturating_add(CHUNK_BYTES));
        while !text.is_char_boundary(end) { end -= 1; }
        Ok(json!({"id":id,"offset":offset,"nextOffset":(end < text.len()).then_some(end),
            "total":text.len(),"chunk":&text[start..end]}))
    }

    pub(crate) fn release(&self, scope: &str) {
        let mut state = self.state();
        if let Some(ids) = state.scopes.remove(scope) {
            for id in ids {
                if let Some(blob) = state.blobs.get_mut(&id) { blob.pins -= 1; }
            }
        }
    }

    pub(crate) fn clear(&self) {
        let _writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        *self.state() = State::default();
    }
}

pub(crate) struct Projection<'a> {
    pages: &'a Pages,
    _writer: MutexGuard<'a, ()>,
    references: HashSet<String>,
    created: HashSet<String>,
    committed: bool,
}

impl Projection<'_> {
    pub(crate) fn asset(&mut self, text: Arc<str>) -> Result<Value, String> {
        self.put("asset", text, Vec::new())
    }

    pub(crate) fn encode(&mut self, value: &Value) -> Result<Value, String> {
        super::codec::encode(self, value)
    }

    pub(super) fn text(&mut self, text: &str) -> Result<Value, String> {
        self.put("text", Arc::from(text), Vec::new())
    }

    pub(super) fn page(&mut self, kind: &'static str, value: Value) -> Result<Value, String> {
        let mut dependencies = Vec::new();
        collect_references(&value, &mut dependencies)?;
        let bytes = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        self.put(kind, Arc::from(bytes), dependencies)
    }

    fn put(&mut self, kind: &'static str, text: Arc<str>, dependencies: Vec<Reference>) -> Result<Value, String> {
        if text.len() > self.pages.limits.blob_bytes {
            return Err("Game page exceeds the blob size limit".into());
        }
        let id = format!("sha256:{:x}", Sha256::digest(text.as_bytes()));
        let bytes = text.len();
        {
            let mut state = self.pages.state();
            if !state.blobs.contains_key(&id) {
                if bytes > self.pages.limits.bytes || self.pages.limits.blobs == 0 {
                    return Err("Game page cache capacity exceeded".into());
                }
                while state.bytes + bytes > self.pages.limits.bytes
                    || state.blobs.len() >= self.pages.limits.blobs
                {
                    let victim = state.blobs.iter()
                        .filter(|(id, blob)| blob.pins == 0 && !state.provisional.contains(*id))
                        .min_by_key(|(_, blob)| blob.touched).map(|(id, _)| id.clone())
                        .ok_or("Game page cache capacity exceeded")?;
                    let removed = state.blobs.remove(&victim).expect("selected cache entry");
                    state.bytes -= removed.text.len();
                }
                state.clock = state.clock.wrapping_add(1);
                let touched = state.clock;
                state.bytes += bytes;
                state.blobs.insert(id.clone(), Blob {
                    text, dependencies: HashMap::new(), pins: 0, touched,
                });
                self.created.insert(id.clone());
            }
            state.blobs.get_mut(&id).expect("inserted cache entry")
                .dependencies.entry(kind).or_insert(dependencies);
        }
        self.retain(Reference { id: id.clone(), kind })?;
        Ok(json!({"$gamePage":{"id":id,"bytes":bytes,"kind":kind}}))
    }

    pub(super) fn retain_node(&mut self, value: &Value) -> Result<bool, String> {
        match parse_reference(value)? {
            None => Ok(false),
            Some((reference, bytes)) => {
                {
                    let state = self.pages.state();
                    let blob = state.blobs.get(&reference.id).ok_or("Game page has expired or was not issued")?;
                    if blob.text.len() != bytes { return Err("Invalid game page byte count".into()); }
                }
                self.retain(reference)?;
                Ok(true)
            }
        }
    }

    fn retain(&mut self, reference: Reference) -> Result<(), String> {
        // Copy only child metadata under the cache lock; traversal never parses blobs.
        let mut pending = vec![reference];
        let mut visited = HashSet::new();
        while let Some(reference) = pending.pop() {
            if !visited.insert((reference.id.clone(), reference.kind)) { continue; }
            let mut state = self.pages.state();
            let blob = state.blobs.get(&reference.id).ok_or("Game page dependency has expired")?;
            let children = blob.dependencies.get(reference.kind)
                .ok_or("Game page kind was not issued")?.clone();
            state.provisional.insert(reference.id.clone());
            self.references.insert(reference.id);
            drop(state);
            pending.extend(children);
        }
        Ok(())
    }

    pub(crate) fn commit(mut self, scope: &str) -> Result<(), String> {
        let mut state = self.pages.state();
        if !state.scopes.contains_key(scope) && state.scopes.len() >= self.pages.limits.scopes {
            return Err("Game page scope capacity exceeded".into());
        }
        if self.references.iter().any(|id| !state.blobs.contains_key(id)) {
            return Err("Game page dependency has expired".into());
        }
        // Pin the replacement before releasing the previous baseline.
        for id in &self.references {
            state.blobs.get_mut(id).expect("validated cache entry").pins += 1;
        }
        let replacement = std::mem::take(&mut self.references);
        if let Some(previous) = state.scopes.insert(scope.into(), replacement) {
            for id in previous {
                if let Some(blob) = state.blobs.get_mut(&id) { blob.pins -= 1; }
            }
        }
        state.provisional.clear();
        self.committed = true;
        Ok(())
    }
}

impl Drop for Projection<'_> {
    fn drop(&mut self) {
        if self.committed { return; }
        let mut state = self.pages.state();
        state.provisional.clear();
        for id in &self.created {
            if state.blobs.get(id).is_some_and(|blob| blob.pins == 0) {
                let removed = state.blobs.remove(id).expect("uncommitted cache entry");
                state.bytes -= removed.text.len();
            }
        }
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.len() == 71 && id.starts_with("sha256:")
        && id.as_bytes()[7..].iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    { Ok(()) } else { Err("Invalid game page identifier".into()) }
}

fn parse_reference(value: &Value) -> Result<Option<(Reference, usize)>, String> {
    let Some(object) = value.as_object() else { return Ok(None); };
    let Some(reference) = object.get("$gamePage") else { return Ok(None); };
    let reference = reference.as_object().ok_or("Invalid game page reference")?;
    if object.len() != 1 || reference.len() != 3 { return Err("Invalid game page reference".into()); }
    let id = reference.get("id").and_then(Value::as_str).ok_or("Invalid game page identifier")?;
    validate_id(id)?;
    let kind = match reference.get("kind").and_then(Value::as_str) {
        Some("asset") => "asset", Some("text") => "text", Some("array") => "array", Some("object") => "object",
        _ => return Err("Invalid game page kind".into()),
    };
    let bytes = reference.get("bytes").and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok()).ok_or("Invalid game page byte count")?;
    Ok(Some((Reference { id: id.into(), kind }, bytes)))
}

fn collect_references(value: &Value, references: &mut Vec<Reference>) -> Result<(), String> {
    if let Some((reference, _)) = parse_reference(value)? { references.push(reference); return Ok(()); }
    match value {
        Value::Array(items) => for item in items { collect_references(item, references)?; },
        Value::Object(entries) => for item in entries.values() { collect_references(item, references)?; },
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "pages_tests.rs"]
mod tests;
