use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Key {
    path: PathBuf,
    modified: SystemTime,
    len: u64,
}

struct Entry {
    preview: String,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    clock: u64,
}

impl Cache {
    fn get(&mut self, key: &Key) -> Option<String> {
        self.clock = self.clock.wrapping_add(1);
        let used = self.clock;
        let entry = self.entries.get_mut(key)?;
        entry.used = used;
        Some(entry.preview.clone())
    }

    fn insert(&mut self, key: Key, preview: String) {
        self.clock = self.clock.wrapping_add(1);
        let used = self.clock;
        self.entries.retain(|existing, _| existing.path != key.path);
        if preview.len() > MAX_BYTES {
            return;
        }
        let mut bytes = self
            .entries
            .values()
            .map(|entry| entry.preview.len())
            .sum::<usize>();
        while self.entries.len() >= MAX_ENTRIES || bytes + preview.len() > MAX_BYTES {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            {
                if let Some(removed) = self.entries.remove(&oldest) {
                    bytes -= removed.preview.len();
                }
            }
        }
        self.entries.insert(key, Entry { preview, used });
    }
}

fn global() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Lookup only: callers on the status path must never prepare an image.
pub(super) fn get(path: &Path, modified: SystemTime, len: u64) -> Option<String> {
    global()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&Key {
            path: path.to_path_buf(),
            modified,
            len,
        })
}

pub(in super::super) fn get_or_prepare(
    path: &Path,
    modified: SystemTime,
    len: u64,
    prepare: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    let key = Key {
        path: path.to_path_buf(),
        modified,
        len,
    };
    // Check the cache under lock, then run preparation without it. Insertion is serialized.
    if let Some(preview) = get(path, modified, len) {
        return Ok(preview);
    }

    let preview = prepare()?;
    let mut cache = global().lock().unwrap_or_else(|poison| poison.into_inner());
    cache.insert(key, preview.clone());
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn stamp(n: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(n)
    }

    #[test]
    fn unchanged_stamp_reuses_preview_and_changed_stamp_prepares_again() {
        let path = Path::new("cache-test-unchanged");
        let count = AtomicUsize::new(0);
        let make = || {
            get_or_prepare(path, stamp(1), 12, || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok("preview".into())
            })
        };
        assert_eq!(make().unwrap(), "preview");
        assert_eq!(make().unwrap(), "preview");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(get(path, stamp(1), 12).as_deref(), Some("preview"));
        assert!(get(path, stamp(2), 12).is_none());
        assert!(get(path, stamp(1), 13).is_none());
        assert_eq!(count.load(Ordering::SeqCst), 1);

        get_or_prepare(path, stamp(2), 12, || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok("changed".into())
        })
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(get(path, stamp(2), 12).as_deref(), Some("changed"));
        assert!(get(path, stamp(1), 12).is_none());
    }

    #[test]
    fn absent_stamp_cannot_hit_and_cache_evicts_old_entries() {
        let path = Path::new("cache-test-eviction");
        get_or_prepare(path, stamp(3), 10, || Ok("old".into())).unwrap();
        let count = AtomicUsize::new(0);
        // A missing source is handled by the caller before this cache API is invoked.
        // Different path identity cannot reuse a cached preview.
        get_or_prepare(Path::new("cache-test-deleted"), stamp(3), 10, || {
            count.fetch_add(1, Ordering::SeqCst);
            Err("missing".into())
        })
        .expect_err("failed preparation must not return a cached value");
        assert_eq!(count.load(Ordering::SeqCst), 1);

        let mut cache = Cache::default();
        for index in 0..=MAX_ENTRIES {
            let key = Key {
                path: PathBuf::from(format!("evict-{index}")),
                modified: stamp(1),
                len: 1,
            };
            cache.insert(key, "x".into());
        }
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
    }
    #[test]
    fn high_resolution_previews_have_a_total_byte_budget() {
        let mut cache = Cache::default();
        for index in 0..3 {
            cache.insert(
                Key {
                    path: format!("large-{index}").into(),
                    modified: stamp(1),
                    len: 1,
                },
                "x".repeat(3 * 1024 * 1024),
            );
        }
        assert_eq!(cache.entries.len(), 2);
        assert!(
            cache
                .entries
                .values()
                .map(|entry| entry.preview.len())
                .sum::<usize>()
                <= MAX_BYTES
        );
        assert!(cache
            .entries
            .keys()
            .all(|key| key.path != Path::new("large-0")));
    }
}
