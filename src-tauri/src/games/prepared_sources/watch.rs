//! Bounded, native filesystem notifications for prepared artwork sources.
use notify::{
    event::{MetadataKind, ModifyKind},
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, WatcherKind,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const MAX_TARGETS: usize = 2048;
const MAX_DIRECTORIES: usize = 64;
const REGISTRATION_ATTEMPTS: usize = 3;
const MAX_ERROR_BYTES: usize = 1024;
pub(in crate::games) type ChildFilter = fn(EventKind, &std::ffi::OsStr) -> bool;
type Changed = Arc<dyn Fn(Result<(), String>) + Send + Sync>;

#[derive(Debug, Default)]
struct TargetFilter {
    paths: BTreeSet<PathBuf>,
    directories: BTreeSet<PathBuf>,
    children: Option<(PathBuf, ChildFilter)>,
}

pub(in crate::games) struct Watch {
    watcher: RecommendedWatcher,
    changed: Changed,
}

impl Watch {
    pub(in crate::games) fn new(changed: Changed) -> Result<Self, String> {
        let watcher = native_watcher(TargetFilter::default(), changed.clone())?;
        Ok(Self {
            watcher,
            changed,
        })
    }

    /// Run on the source worker, before its source baseline is read.
    /// Register the complete replacement before retiring the old native watch.
    pub(in crate::games) fn replace(&mut self, paths: &[PathBuf]) -> Result<(), String> {
        self.replace_with_children(paths, None)
    }

    pub(in crate::games) fn replace_with_children(
        &mut self, paths: &[PathBuf], children: Option<(&Path, ChildFilter)>,
    ) -> Result<(), String> {
        let (mut targets, mut directories) = configuration(paths)?;
        let children = if let Some((root, filter)) = children {
            let root = comparison_path(root);
            if !targets.paths.contains(&root) {
                return Err("The child watch root must be an exact source target.".into());
            }
            Some((root, filter))
        } else { None };
        for _ in 0..REGISTRATION_ATTEMPTS {
            let watched_directories = targets.directories.clone();
            targets.children = children.clone();
            let mut replacement = native_watcher(targets, self.changed.clone())?;
            for directory in directories.values() {
                replacement.watch(directory, RecursiveMode::NonRecursive).map_err(|error| {
                    bounded(format!("Cannot watch artwork source {}: {error}", directory.display()))
                })?;
            }
            // A missing parent can appear between resolving its nearest
            // ancestor and arming the watch. Baseline only after the exact
            // directory set is covered by active handles.
            let (current_targets, current_directories) = configuration(paths)?;
            if current_directories != directories || current_targets.directories != watched_directories {
                targets = current_targets;
                directories = current_directories;
                continue;
            }
            self.watcher = replacement;
            return Ok(());
        }
        Err("Artwork source directories changed during native watch registration.".into())
    }
}

fn native_watcher(targets: TargetFilter, changed: Changed) -> Result<RecommendedWatcher, String> {
    if matches!(
        RecommendedWatcher::kind(),
        WatcherKind::PollWatcher | WatcherKind::NullWatcher
    ) {
        return Err("Native artwork source notifications are unsupported on this platform.".into());
    }
    notify::recommended_watcher(move |event| dispatch(event, &targets, &changed)).map_err(|error| {
        bounded(format!(
            "Cannot start native artwork source notifications: {error}"
        ))
    })
}

fn dispatch(event: notify::Result<Event>, targets: &TargetFilter, changed: &Changed) {
    let event = match event {
        Err(error) => {
            changed(Err(bounded(format!(
                "Artwork source notifications were lost: {error}"
            ))));
            return;
        }
        Ok(event) if event.need_rescan() => {
            changed(Err(
                "Artwork source notifications were lost; a new source baseline is required.".into(),
            ));
            return;
        }
        Ok(event) => event,
    };
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    if event.paths.iter().any(|path| {
        let path = comparison_path(path);
        // ReadDirectoryChangesW reports directory last-write changes when an
        // unrelated child is written. Only directory replacement events matter
        // here; selected file writes still invalidate their exact file target.
        if matches!(
            event.kind,
            EventKind::Modify(
                ModifyKind::Any
                    | ModifyKind::Data(_)
                    | ModifyKind::Metadata(MetadataKind::WriteTime)
            )
        ) && targets.directories.contains(&path)
        {
            return false;
        }
        targets.paths.iter().any(|target| target.starts_with(&path))
            || targets.children.as_ref().is_some_and(|(root, filter)| {
                path.parent() == Some(root.as_path())
                    && path.file_name().is_some_and(|name| filter(event.kind, name))
            })
    }) {
        changed(Ok(()));
    }
}

fn configuration(paths: &[PathBuf]) -> Result<(TargetFilter, BTreeMap<PathBuf, PathBuf>), String> {
    if paths.len() > MAX_TARGETS {
        return Err(format!(
            "Artwork source target limit exceeded ({MAX_TARGETS})."
        ));
    }
    let mut targets = TargetFilter::default();
    let mut directories = BTreeMap::new();
    for path in paths {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(
                "Artwork source targets must be absolute paths without parent traversal.".into(),
            );
        }
        targets.paths.insert(comparison_path(path));
        if metadata(path)?.is_some_and(|metadata| metadata.is_dir()) {
            targets.directories.insert(comparison_path(path));
            insert_directory(path, &mut directories)?;
        }
        if let Some(parent) = path.parent() {
            insert_directory(&nearest_directory(parent)?, &mut directories)?;
        } else {
            // A filesystem root has no parent; watch the root itself.
            insert_directory(&nearest_directory(path)?, &mut directories)?;
        }
    }
    targets.directories.extend(directories.keys().cloned());
    // Missing source parents are directories too, before their first creation
    // has caused the worker to register a new baseline and native watch set.
    for path in &targets.paths {
        for ancestor in path.ancestors().skip(1) {
            if targets.paths.contains(ancestor) {
                targets.directories.insert(ancestor.to_owned());
            }
        }
    }
    Ok((targets, directories))
}

fn metadata(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(bounded(format!(
            "Cannot inspect artwork source {}: {error}",
            path.display()
        ))),
    }
}

fn nearest_directory(path: &Path) -> Result<PathBuf, String> {
    for ancestor in path.ancestors() {
        if let Some(metadata) = metadata(ancestor)? {
            return if metadata.is_dir() {
                Ok(ancestor.to_owned())
            } else {
                Err(bounded(format!(
                    "Artwork source parent is not a directory: {}",
                    ancestor.display()
                )))
            };
        }
    }
    Err(bounded(format!(
        "Artwork source has no accessible parent: {}",
        path.display()
    )))
}

fn insert_directory(
    path: &Path,
    directories: &mut BTreeMap<PathBuf, PathBuf>,
) -> Result<(), String> {
    directories
        .entry(comparison_path(path))
        .or_insert_with(|| path.to_owned());
    if directories.len() > MAX_DIRECTORIES {
        return Err(format!(
            "Artwork source watch directory limit exceeded ({MAX_DIRECTORIES})."
        ));
    }
    Ok(())
}

fn bounded(mut message: String) -> String {
    if message.len() > MAX_ERROR_BYTES {
        let mut end = MAX_ERROR_BYTES;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    message
}

#[cfg(not(windows))]
fn comparison_path(path: &Path) -> PathBuf {
    path.components().collect()
}

#[cfg(windows)]
fn comparison_path(path: &Path) -> PathBuf {
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
    };
    // Canonical paths use the extended prefix, while native events may omit it.
    // Preserve unpaired UTF-16 surrogates instead of comparing lossy path strings.
    let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let mut prefixless = units.as_slice();
    let mut unc = false;
    if prefixless.starts_with(&[92, 92, 63, 92]) {
        prefixless = &prefixless[4..];
        if prefixless.len() >= 4
            && prefixless[..4]
                .iter()
                .zip([85, 78, 67, 92])
                .all(|(actual, expected)| *actual == expected || *actual == expected + 32)
        {
            prefixless = &prefixless[4..];
            unc = true;
        }
    }
    let mut folded = if unc { vec![92, 92] } else { Vec::new() };
    for value in char::decode_utf16(prefixless.iter().copied()) {
        match value {
            Ok(character) => {
                for lower in character.to_lowercase() {
                    folded.extend_from_slice(lower.encode_utf16(&mut [0; 2]));
                }
            }
            Err(error) => folded.push(error.unpaired_surrogate()),
        }
    }
    Path::new(&OsString::from_wide(&folded))
        .components()
        .collect()
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
