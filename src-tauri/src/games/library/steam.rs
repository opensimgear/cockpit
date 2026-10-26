use super::*;

fn steam_art(root: &Path, app_id: u32) -> Option<PathBuf> {
    let cache = root.join("appcache/librarycache");
    let mut candidates = [
        format!("{app_id}_library_hero.jpg"),
        format!("{app_id}_header.jpg"),
        format!("{app_id}_header_2x.jpg"),
        format!("{app_id}_library_600x900.jpg"),
    ]
    .into_iter()
    .map(|relative| cache.join(relative))
    .collect::<Vec<_>>();
    // Current Steam uses app-id/content-hash/library_header.jpg. Stay inside
    // that app's cache and inspect only one bounded level of hash directories.
    let app = cache.join(app_id.to_string());
    if regular(&app, true) {
        for path in sorted_children(&app).into_iter().take(128) {
            if regular(&path, true)
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        matches!(name.len(), 32 | 40 | 64)
                            && name.bytes().all(|b| b.is_ascii_hexdigit())
                    })
            {
                candidates.extend(sorted_children(&path).into_iter().take(32));
            } else {
                candidates.push(path);
            }
        }
    }
    candidates
        .into_iter()
        .filter_map(|path| steam_art_rank(&path).map(|rank| (rank, path)))
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, path)| path)
}

/// Return every recognizable raster image inside one app's Steam cache.
/// The app-id directory is the identity boundary; neighboring app caches are
/// never traversed. Both modern hash folders and older flat names are read.
pub(super) fn steam_art_choices(root: &Path, app_id: u32) -> Vec<PathBuf> {
    let cache = root.join("appcache/librarycache");
    let mut candidates = Vec::new();
    for name in [
        "library_hero",
        "header",
        "header_2x",
        "library_600x900",
        "library_capsule",
        "library_logo",
        "logo",
        "icon",
        "library_header",
        "library_capsule_2x",
    ] {
        for extension in ["jpg", "png", "webp"] {
            candidates.push(cache.join(format!("{app_id}_{name}.{extension}")));
        }
    }
    let app = cache.join(app_id.to_string());
    if regular(&app, true) {
        for path in sorted_children(&app).into_iter().take(128) {
            if regular(&path, true)
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        matches!(name.len(), 32 | 40 | 64)
                            && name.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
            {
                candidates.extend(sorted_children(&path).into_iter().take(64));
            } else {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
        .into_iter()
        .filter(|path| steam_art_name(path) && super::artwork_choices::valid_raster(path))
        .take(64)
        .collect()
}

fn steam_art_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    !name.contains("blur")
        && ["header", "hero", "capsule", "600x900", "logo", "icon"]
            .iter()
            .any(|kind| name.contains(kind))
}

fn steam_art_rank(path: &Path) -> Option<(u64, bool)> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if !regular(path, false)
        || name.contains("blur")
        || !(name.contains("header")
            || name.contains("hero")
            || name.contains("capsule")
            || name.contains("600x900"))
        || !matches!(path.extension()?.to_str()?, "jpg" | "png" | "webp")
        || fs::metadata(path).ok()?.len() > 16 * 1024 * 1024
    {
        return None;
    }
    let (width, height) = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return None;
    }
    // Compare pixels remaining after our 960px preview and 16:9 card crop.
    // This lets a 920px header beat a panoramic hero or a tall cover when sharper.
    let edge = width.max(height).max(960) as u64;
    let crop_width = (width as u64 * 960 / edge).min(height as u64 * 960 * 16 / edge / 9);
    Some((
        crop_width,
        name.contains("header") || (name.contains("capsule") && width > height),
    ))
}
pub(super) fn steam_candidates(roots: &[PathBuf], scan: &mut Scan) {
    let mut seen = BTreeSet::new();
    for root in roots.iter().take(16) {
        let launcher = regular(&root.join("steam.exe"), false)
            .then(|| canonical_text(&root.join("steam.exe")))
            .flatten();
        for library in game_discovery::library_paths(root).into_iter().take(32) {
            for path in sorted_children(&library.join("steamapps")) {
                if scan.candidates.len() >= MAX_CANDIDATES {
                    scan.diagnostic("The local library scan reached its 512-entry limit.".into());
                    return;
                }
                if !regular(&path, false)
                    || !path.file_name().is_some_and(|name| {
                        let name = name.to_string_lossy();
                        name.starts_with("appmanifest_") && name.ends_with(".acf")
                    })
                {
                    continue;
                }
                let Some(manifest) = game_discovery::read_vdf(&path) else {
                    scan.diagnostic(format!(
                        "Steam manifest could not be read: {}",
                        path.display()
                    ));
                    continue;
                };
                let Some(state) = unique_vdf(&manifest, "AppState") else {
                    continue;
                };
                let Some(app_id) = unique_vdf(state, "appid")
                    .and_then(Vdf::text)
                    .and_then(|text| text.parse::<u32>().ok())
                    .filter(|id| *id > 0)
                else {
                    continue;
                };
                let filename_id = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_prefix("appmanifest_"))
                    .and_then(|name| name.strip_suffix(".acf"))
                    .and_then(|id| id.parse::<u32>().ok());
                if filename_id != Some(app_id) {
                    scan.diagnostic(format!(
                        "Steam manifest app identity does not match its filename: {}",
                        path.display()
                    ));
                    continue;
                }
                let Some(directory) = unique_vdf(state, "installdir")
                    .and_then(Vdf::text)
                    .filter(|value| safe_component(value))
                else {
                    continue;
                };
                let install = library.join("steamapps/common").join(directory);
                if !regular(&install, true) {
                    continue;
                }
                let Some(install_path) = canonical_text(&install) else {
                    continue;
                };
                if !seen.insert(app_id) {
                    continue;
                }
                let name = unique_vdf(state, "name")
                    .and_then(Vdf::text)
                    .and_then(clean_name)
                    .or_else(|| clean_name(directory))
                    .unwrap_or_else(|| format!("Steam game {app_id}"));
                let kind = if name.to_lowercase().contains("dedicated server")
                    || name.to_lowercase().contains("redistributable")
                    || name.to_lowercase().contains("steamworks")
                {
                    "tool"
                } else {
                    "unknown"
                };
                scan.candidates.push(Candidate {
                    id: format!("steam:{app_id}"),
                    client: "steam".into(),
                    name,
                    install_path,
                    executable_candidates: executable_candidates(&install, None),
                    preferred_executable: None,
                    game_id: game_discovery::recognized_for_steam(app_id).map(str::to_owned),
                    kind: kind.into(),
                    app_id: app_id.to_string(),
                    launcher: launcher.clone(),
                    artwork: steam_art(root, app_id),
                });
            }
        }
    }
}

#[cfg(test)]
mod artwork_tests {
    use super::*;
    #[test]
    fn best_actual_card_resolution_wins_across_flat_and_hashed_cache_layouts() {
        let temp = crate::test_directory();
        let cache = temp.path().join("appcache/librarycache");
        fs::create_dir_all(&cache).unwrap();
        let save = |relative: &str, width, height| {
            let path = cache.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            image::RgbImage::new(width, height).save(&path).unwrap();
            path
        };
        save("42_library_hero.jpg", 1920, 620);
        save("42_library_600x900.jpg", 300, 450);
        save("42/header.jpg", 460, 215);
        let header = save(
            "42/abcdef0123456789abcdef0123456789abcdef01/library_header.jpg",
            920,
            430,
        );
        save(
            "42/abcdef0123456789abcdef0123456789abcdef01/library_hero_blur.jpg",
            1920,
            1080,
        );
        save(
            "42/abcdef0123456789abcdef0123456789abcdef01/logo.png",
            1920,
            1080,
        );
        save("43/header.jpg", 4000, 2000);
        assert_eq!(steam_art(temp.path(), 42), Some(header.clone()));
        fs::remove_file(header).unwrap();
        assert_eq!(
            steam_art(temp.path(), 42),
            Some(cache.join("42_library_hero.jpg"))
        );
        assert!(steam_art(temp.path(), 99).is_none());
    }
}
