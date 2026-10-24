use std::path::{Component, Path, PathBuf};

/// Reject symlinks/reparse points for the database and its parents/sidecars.
pub fn check_path(path: &Path, create: bool) -> Result<(), String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Persistence requires an absolute path without traversal.".into());
    }
    let mut current = PathBuf::new();
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_))
            && matches!(components.peek(), Some(Component::RootDir))
        {
            continue;
        }
        let parent = components.peek().is_some();
        match std::fs::symlink_metadata(&current) {
            Ok(meta) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err("Persistence cannot follow reparse points.".into());
                    }
                }
                if meta.file_type().is_symlink()
                    || (parent && !meta.is_dir())
                    || (!parent && !meta.is_file())
                {
                    return Err("Persistence path is indirect or has the wrong type.".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if parent && create {
                    std::fs::create_dir(&current)
                        .map_err(|_| "Cannot create persistence directory.")?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o700))
                            .map_err(|_| "Cannot protect persistence directory.")?;
                    }
                }
            }
            Err(_) => return Err("Cannot inspect persistence path.".into()),
        }
    }
    Ok(())
}
