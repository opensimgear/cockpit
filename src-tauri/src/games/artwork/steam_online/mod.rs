//! Optional artwork enrichment. Queueing is cheap; network and file I/O run off the status path.
use super::*;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};
mod http;
mod targets;
mod owner;
pub(crate) use owner::Owner;

const MAX_ENTRIES: usize = 512;
const WORKERS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    root: PathBuf,
    app_id: u32,
}
#[derive(Clone)]
enum State {
    Pending,
    Ready(Arc<str>),
    Failed { scan: String, retry: Instant },
}
struct Job {
    key: Key,
    scan: String,
    ticket: u64,
}
#[derive(Default)]
struct Service {
    states: BTreeMap<Key, State>,
    jobs: VecDeque<Job>,
    workers: usize,
    tickets: BTreeMap<Key, u64>,
    next_ticket: u64,
    owners: BTreeMap<u64, owner::Registration>,
    next_owner: u64,
}

fn service() -> &'static Mutex<Service> {
    static SERVICE: OnceLock<Mutex<Service>> = OnceLock::new();
    SERVICE.get_or_init(Default::default)
}

impl Service {
    fn make_room(&mut self, key: &Key) -> bool {
        if self.states.contains_key(key) || self.states.len() < MAX_ENTRIES { return true; }
        let stale = self.states.iter().find(|(_, value)| !matches!(value, State::Pending))
            .map(|(key, _)| key.clone());
        if let Some(stale) = stale {
            owner::changed_key(self, &stale);
            self.states.remove(&stale);
            self.tickets.remove(&stale);
            true
        } else { false }
    }

    /// Explicit disk refresh can succeed even when the optional memory cache is full.
    fn cache_ready(&mut self, key: Key, image: Arc<str>) -> bool {
        if !self.make_room(&key) { return false; }
        self.tickets.remove(&key);
        if !matches!(self.states.get(&key), Some(State::Ready(old)) if old == &image) { owner::changed_key(self, &key); }
        self.states.insert(key.clone(), State::Ready(image));
        owner::refreshed(self, &key);
        true
    }

    fn request(&mut self, key: Key, scan: &str, now: Instant) -> State {
        if let Some(state) = self.states.get(&key) {
            if !matches!(state, State::Failed { scan: previous, retry } if previous != scan || *retry <= now)
            {
                return state.clone();
            }
        }
        if !self.make_room(&key) {
            return State::Failed {
                scan: scan.into(), retry: now + Duration::from_secs(300),
            };
        }
        let Some(ticket) = self.next_ticket.checked_add(1) else {
            return State::Failed { scan: scan.into(), retry: now + Duration::from_secs(300) };
        };
        self.next_ticket = ticket;
        self.tickets.insert(key.clone(), ticket);
        owner::changed_key(self, &key);
        self.states.insert(key.clone(), State::Pending);
        self.jobs.push_back(Job {
            key,
            scan: scan.into(),
            ticket,
        });
        State::Pending
    }
}

/// Schedule optional enrichment from an authoritative prepared snapshot.
pub(crate) fn prepare(root: &Path, state: &Value) {
    prepare_with(service(), root, state, || {
        std::thread::Builder::new().name("steam-artwork".into())
            .spawn(worker).map(|_| ()).map_err(|error| error.to_string())
    });
}

fn prepare_with(pool: &Mutex<Service>, root: &Path, state: &Value, mut spawn: impl FnMut() -> Result<(), String>) {
    if state["discovery"]["status"] == "scanning" {
        return;
    }
    let targets = targets::collect(state);
    let scan = state["discovery"]["scanId"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let mut jobs = pool.lock().unwrap_or_else(|p| p.into_inner());
    for target in targets {
        jobs.request(
            Key {
                root: root.into(),
                app_id: target.app_id,
            },
            &scan,
            Instant::now(),
        );
    }
    let failed = start_workers(&mut jobs, &mut spawn);
    if failed {
        owner::synchronize(&mut jobs);
    }
    let wakes = owner::changed_wakes(&jobs, failed);
    drop(jobs);
    for inner in wakes { inner.notify(); }
}

fn start_workers(jobs: &mut Service, mut spawn: impl FnMut() -> Result<(), String>) -> bool {
    while jobs.workers < WORKERS && jobs.workers < jobs.jobs.len() {
        jobs.workers += 1;
        if spawn().is_err() {
            jobs.workers -= 1;
            while let Some(job) = jobs.jobs.pop_front() {
                jobs.states.insert(
                    job.key,
                    State::Failed {
                        scan: job.scan,
                        retry: Instant::now() + Duration::from_secs(300),
                    },
                );
            }
            return true;
        }
    }
    false
}

/// Passive projection of already prepared in-memory artwork; never schedules work.
pub(crate) fn decorate(root: &Path, state: &mut Value) {
    decorate_with(service(), root, state);
}

fn decorate_with(pool: &Mutex<Service>, root: &Path, state: &mut Value) {
    decorate_assets_with(pool, root, state, &mut |image| Ok(Value::String(image.to_string())))
        .expect("String artwork projection is infallible");
}

pub(crate) fn decorate_assets(root: &Path, state: &mut Value, resolve: &mut impl FnMut(&Arc<str>) -> Result<Value, String>) -> Result<(), String> {
    decorate_assets_with(service(), root, state, resolve)
}

fn decorate_assets_with(pool: &Mutex<Service>, root: &Path, state: &mut Value, resolve: &mut impl FnMut(&Arc<str>) -> Result<Value, String>) -> Result<(), String> {
    if state["discovery"]["status"] == "scanning" { return Ok(()); }
    let targets = targets::collect(state);
    let mut pending = false;
    let cached = {
        let jobs = pool.lock().unwrap_or_else(|p| p.into_inner());
        targets.into_iter().map(|target| {
            let key = Key { root: root.into(), app_id: target.app_id };
            let cached = jobs.states.get(&key).cloned();
            let accepted = owner::pending(&jobs, &key);
            (target, cached, accepted)
        }).collect::<Vec<_>>()
    };
    for (target, cached, accepted) in cached {
        match cached {
            Some(State::Ready(image)) => {
                if let Some(index) = target.game {
                    state["catalogue"][index]["defaultBackground"] =
                        serde_json::json!({"image":resolve(&image)?,"positionX":50,"positionY":50});
                } else if let Some(index) = target.entry {
                    state["discovery"]["entries"][index]["image"] = resolve(&image)?;
                }
            }
            Some(State::Pending) => pending = true,
            Some(State::Failed { .. }) | None => pending |= accepted,
        }
    }
    state["artworkPending"] = pending.into();
    state["discovery"]["artworkPending"] = pending.into();
    Ok(())
}

fn worker() {
    run_worker(service(), |key| {
        cached_or_fetch(&key.root, key.app_id, http::download)
    });
}

fn run_worker(pool: &Mutex<Service>, fetch: impl Fn(&Key) -> Result<String, String>) {
    loop {
        let job = {
            let mut service = pool.lock().unwrap_or_else(|p| p.into_inner());
            loop { match service.jobs.pop_front() {
                Some(job) if service.tickets.get(&job.key) == Some(&job.ticket)
                    && matches!(service.states.get(&job.key), Some(State::Pending)) => break job,
                Some(_) => continue,
                None => {
                    service.workers -= 1;
                    return;
                }
            } }
        };
        let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fetch(&job.key)));
        let result = match image {
            Ok(Ok(image)) => State::Ready(image.into()),
            _ => State::Failed {
                scan: job.scan,
                retry: Instant::now() + Duration::from_secs(300),
            },
        };
        let wakes = {
            let mut service = pool.lock().unwrap_or_else(|p| p.into_inner());
            if service.tickets.get(&job.key) == Some(&job.ticket)
                && matches!(service.states.get(&job.key), Some(State::Pending)) {
                owner::changed_key(&service, &job.key);
                service.states.insert(job.key, result);
                owner::synchronize(&mut service);
            }
            owner::wakes(&service)
        };
        for wake in wakes { wake.notify(); }
    }
}

fn directory(root: &Path) -> Result<PathBuf, String> {
    storage(root)?;
    let directory = root.join("steam-artwork");
    if let Ok(metadata) = fs::symlink_metadata(&directory) {
        if redirected(&metadata) || !metadata.is_dir() {
            return Err("Invalid Steam artwork cache directory.".into());
        }
    }
    Ok(directory)
}

fn cached_or_fetch(
    root: &Path,
    app_id: u32,
    fetch: impl FnOnce(u32) -> Result<String, String>,
) -> Result<String, String> {
    cached_or_fetch_using(root,app_id,fetch,false)
}

/// Explicit refresh for the exact application attached to one game.
pub(crate) fn refresh(root:&Path,app_id:u32)->Option<PathBuf> {
    let image: Arc<str> = cached_or_fetch_using(root,app_id,http::download,true).ok()?.into();
    let wakes = {
        let mut service = service().lock().unwrap_or_else(|p|p.into_inner());
        let key = Key{root:root.into(),app_id};
        service.cache_ready(key, image);
        owner::wakes(&service)
    };
    for wake in wakes { wake.notify(); }
    Some(directory(root).ok()?.join(format!("{app_id}.jpg")))
}

fn cached_or_fetch_using(root:&Path,app_id:u32,fetch:impl FnOnce(u32)->Result<String,String>,refresh:bool)->Result<String,String> {
    if app_id == 0 {
        return Err("Steam artwork needs an app ID.".into());
    }
    let directory = directory(root)?;
    let path = directory.join(format!("{app_id}.jpg"));
    let mut fallback = None;
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if redirected(&metadata) || !metadata.is_file() {
            return Err("Invalid Steam artwork cache file.".into());
        }
        if metadata.len() <= DISCOVERY_PREVIEW_LIMIT as u64 {
            if let Ok(bytes) = fs::File::open(&path).and_then(|file| {
                let mut bytes = Vec::new();
                file.take(DISCOVERY_PREVIEW_LIMIT as u64 + 1)
                    .read_to_end(&mut bytes)?;
                Ok(bytes)
            }) {
                let image = format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes));
                if let Ok(bytes) = discovery_preview_bytes(&image) {
                    if !refresh&&decoded(&bytes).is_ok_and(|image| image.width().max(image.height()) > 384) {
                        return Ok(image);
                    }
                    fallback = Some(image);
                }
            }
        }
    }
    let fetched =
        fetch(app_id).and_then(|image| discovery_preview_bytes(&image).map(|bytes| (image, bytes)));
    let (image, bytes) = match fetched {
        Ok(value) => value,
        Err(error) => return fallback.ok_or(error),
    };
    // Keep a valid offline image if the server only supplies an even smaller original.
    if let Some(previous) = fallback {
        let old = decoded(&discovery_preview_bytes(&previous)?)?;
        let new = decoded(&bytes)?;
        if new.width().saturating_mul(new.height()) < old.width().saturating_mul(old.height())
            || !refresh&&new.width().saturating_mul(new.height()) == old.width().saturating_mul(old.height()) {
            return Ok(previous);
        }
    }
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    if !path.exists()
        && fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .take(MAX_ENTRIES + 1)
            .count()
            >= MAX_ENTRIES
    {
        return Err("Steam artwork cache reached its image limit.".into());
    }
    let mut temporary =
        tempfile::NamedTempFile::new_in(&directory).map_err(|error| error.to_string())?;
    temporary
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    temporary
        .persist(&path)
        .map_err(|error| error.to_string())?;
    Ok(image)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod read_model_tests;
#[cfg(test)]
#[path = "asset_stream_tests.rs"]
mod asset_stream_tests;
