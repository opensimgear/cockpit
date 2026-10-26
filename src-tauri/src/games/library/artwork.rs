use super::{Path, PathBuf, regular};

/// Common local capsules and Xbox package visuals; no remote requests or guesses
/// at a game's identity are needed to use artwork shipped with its installation.
pub(super) fn local(root: &Path) -> Option<PathBuf> {
    [
        "cover.jpg",
        "cover.png",
        "header.jpg",
        "folder.jpg",
        "game.png",
        "Assets/StoreLogo.png",
        "Assets/Square150x150Logo.png",
        "Content/Assets/StoreLogo.png",
        "Content/Assets/Square150x150Logo.png",
    ]
    .iter()
    .map(|relative| root.join(relative))
    .find(|path| regular(path, false))
}

pub(super) fn local_choices(root: &Path) -> Vec<PathBuf> {
    [
        "cover.jpg",
        "cover.png",
        "cover.webp",
        "header.jpg",
        "header.png",
        "header.webp",
        "folder.jpg",
        "folder.png",
        "game.png",
        "game.jpg",
        "Assets/StoreLogo.png",
        "Assets/Square150x150Logo.png",
        "Content/Assets/StoreLogo.png",
        "Content/Assets/Square150x150Logo.png",
    ]
    .iter()
    .map(|relative| root.join(relative))
    .filter(|path| regular(path, false) && super::artwork_choices::valid_raster(path))
    .take(64)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_present_regular_named_artwork_is_suggested() {
        let directory = crate::test_directory();
        assert!(local(directory.path()).is_none());
        std::fs::create_dir(directory.path().join("cover.jpg")).unwrap();
        assert!(local(directory.path()).is_none());
        let assets = directory.path().join("Content/Assets");
        std::fs::create_dir_all(&assets).unwrap();
        let image = assets.join("StoreLogo.png");
        std::fs::write(
            &image,
            b"Validated as an image only when a preview is prepared.",
        )
        .unwrap();
        assert_eq!(local(directory.path()), Some(image));
        std::fs::write(directory.path().join("header.jpg"), b"header").unwrap();
        assert_eq!(
            local(directory.path()),
            Some(directory.path().join("header.jpg"))
        );
    }
}
