use super::super::{Entry, Review};
use std::{path::Path, sync::Mutex};

const ARTWORK_BUDGET: usize = 2 * 1024 * 1024;

/// Matching is complete before thumbnails are prepared. Publish the rows now
/// so slow artwork cannot leave a completed library looking empty.
pub(super) fn publish_entries(state: &Mutex<Review>, scan_id: &str, entries: Vec<Entry>) -> bool {
    let mut review = state.lock().unwrap_or_else(|p| p.into_inner());
    if review.scan_id != scan_id || review.status != "scanning" {
        return false;
    }
    review.entries = entries;
    true
}

pub(super) fn prepare_artwork(
    state: &Mutex<Review>,
    scan_id: &str,
    closed: impl Fn() -> bool,
    mut preview: impl FnMut(&Path) -> Option<String>,
) -> bool {
    let sources = {
        let review = state.lock().unwrap_or_else(|p| p.into_inner());
        if review.scan_id != scan_id || review.status != "scanning" {
            return false;
        }
        review
            .entries
            .iter()
            .filter_map(|entry| {
                entry
                    .artwork
                    .as_ref()
                    .map(|path| (entry.id.clone(), path.clone()))
            })
            .collect::<Vec<_>>()
    };
    let mut bytes = 0;
    for (id, path) in sources {
        if closed() {
            return false;
        }
        if bytes >= ARTWORK_BUDGET {
            break;
        }
        // File I/O and image decoding never hold the discovery status lock.
        let image = preview(&path).filter(|image| bytes + image.len() <= ARTWORK_BUDGET);
        let mut review = state.lock().unwrap_or_else(|p| p.into_inner());
        if review.scan_id != scan_id || review.status != "scanning" {
            return false;
        }
        if let Some(entry) = review.entries.iter_mut().find(|entry| entry.id == id) {
            bytes += image.as_ref().map_or(0, String::len);
            entry.image = image;
        }
    }
    if closed() {
        return false;
    }
    let mut review = state.lock().unwrap_or_else(|p| p.into_inner());
    if review.scan_id != scan_id || review.status != "scanning" {
        return false;
    }
    for library in &mut review.libraries {
        if library.status == "preparing" {
            library.status = "complete".into();
        }
    }
    review.status = "ready".into();
    true
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;
