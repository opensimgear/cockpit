use super::super::{Entry, Review};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

const ARTWORK_BUDGET: usize = 8 * 1024 * 1024;

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

#[cfg(test)]
pub(super) fn prepare_artwork(
    state: &Mutex<Review>,
    scan_id: &str,
    closed: impl Fn() -> bool,
    preview: impl FnMut(&Path) -> Option<String>,
) -> bool {
    prepare_artwork_with_sources(state, scan_id, &[], closed, preview)
}

pub(super) fn prepare_artwork_with_sources(
    state: &Mutex<Review>,
    scan_id: &str,
    extra_sources: &[PathBuf],
    closed: impl Fn() -> bool,
    mut preview: impl FnMut(&Path) -> Option<String>,
) -> bool {
    let mut sources = {
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
                    .map(|path| (Some(entry.id.clone()), path.clone()))
            })
            .collect::<Vec<_>>()
    };
    for path in extra_sources {
        if !sources.iter().any(|(_, source)| source == path) {
            sources.push((None, path.clone()));
        }
    }
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
        bytes += image.as_ref().map_or(0, String::len);
        let mut review = state.lock().unwrap_or_else(|p| p.into_inner());
        if review.scan_id != scan_id || review.status != "scanning" {
            return false;
        }
        if let Some(entry) = review
            .entries
            .iter_mut()
            .find(|entry| Some(&entry.id) == id.as_ref())
        {
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
