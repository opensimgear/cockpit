use super::*;
pub(super) fn reconcile(
    saved: &crate::game_installations::Settings,
    candidates: Vec<Candidate>,
    successful: &BTreeSet<String>,
) -> Vec<Entry> {
    let mut grouped: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for candidate in candidates
        .into_iter()
        .filter(|candidate| candidate.kind != "tool")
    {
        grouped
            .entry(crate::game_discovery::normalized_path(
                &candidate.install_path,
            ))
            .or_default()
            .push(candidate);
    }
    let mut entries = vec![];
    let mut replaced = BTreeSet::new();
    for (_, mut observations) in grouped {
        observations
            .sort_by_key(|candidate| (candidate.game_id.is_none(), candidate.client == "disk"));
        let candidate = &observations[0];
        let root = Path::new(&candidate.install_path);
        let recognition =
            crate::game_discovery::recognize_installation(root, &candidate.executable_candidates);
        let recognized = candidate
            .game_id
            .clone()
            .or_else(|| recognition.as_ref().map(|(id, _, _)| id.clone()));
        let integration = recognized
            .as_ref()
            .filter(|id| crate::game_discovery::integration_provider(id).is_some())
            .cloned();
        let existing_root = saved.games.iter().find_map(|(game_id, game)| {
            game.installations
                .iter()
                .find(|(_, installation)| same_path(&installation.root, &candidate.install_path))
                .map(|(id, installation)| (game_id.clone(), id.clone(), installation))
        });
        let source_match = saved.games.iter().find_map(|(game_id, game)| {
            game.installations
                .iter()
                .find(|(_, installation)| {
                    installation.sources.iter().any(|old| {
                        observations
                            .iter()
                            .any(|new| old.client == new.client && old.app_id == new.app_id)
                    })
                })
                .map(|(id, installation)| (game_id.clone(), id.clone(), installation))
        });
        let game_id = existing_root
            .as_ref()
            .map(|(id, _, _)| id.clone())
            .or(recognized.clone())
            .or_else(|| source_match.as_ref().map(|(id, _, _)| id.clone()))
            .unwrap_or_else(|| {
                key(
                    "game",
                    &format!("{}:{}", candidate.client, candidate.app_id),
                )
            });
        let sources = observations
            .iter()
            .map(Candidate::source)
            .collect::<Vec<_>>();
        let mut runtimes = vec![];
        for observation in &observations {
            for path in &observation.executable_candidates {
                if !runtimes.contains(path) && runtimes.len() < 32 {
                    runtimes.push(path.clone());
                }
            }
        }
        let moved = source_match.as_ref().filter(|(game, id, old)| {
            absent(&old.root) && !replaced.contains(&(game.clone(), id.clone()))
        });
        let (installation_id, previous_path, kind) = if let Some((_, id, old)) = &existing_root {
            let unchanged = integration.as_ref().is_none_or(|key| {
                saved
                    .games
                    .get(&game_id)
                    .and_then(|game| game.integration.as_ref())
                    == Some(key)
            }) && sources.iter().all(|source| old.sources.contains(source))
                && runtimes == old.executables
                && old
                    .executable
                    .as_ref()
                    .is_none_or(|path| Path::new(path).is_file());
            if unchanged {
                continue;
            }
            (id.clone(), Some(old.root.clone()), "refresh")
        } else if let Some((_, id, old)) = moved {
            replaced.insert((game_id.clone(), id.clone()));
            (id.clone(), Some(old.root.clone()), "replace-path")
        } else {
            (
                key(
                    "installation",
                    &crate::game_discovery::normalized_path(&candidate.install_path),
                ),
                None,
                "add-installation",
            )
        };
        let action = if saved.games.contains_key(&game_id) {
            "update"
        } else {
            "add"
        };
        let name = saved
            .games
            .get(&game_id)
            .map(|game| game.name.clone())
            .or_else(|| recognition.map(|(_, name, _)| name))
            .unwrap_or_else(|| candidate.name.clone());
        let recognized_executable = recognized
            .as_deref()
            .and_then(|id| {
                runtimes.iter().find(|path| {
                    crate::game_discovery::recognize_installation(Path::new(path), &[])
                        .is_some_and(|(recognized_id, _, _)| recognized_id == id)
                })
            })
            .map(String::as_str);
        let preferred_executable = observations
            .iter()
            .find_map(|observation| observation.preferred_executable.as_deref())
            .filter(|path| runtimes.iter().any(|runtime| same_path(runtime, path)));
        let evidenced = recognized_executable.or(preferred_executable);
        let ranked = crate::game_library::executable_ranking::rank(&name, &runtimes, evidenced);
        let executable = existing_root
            .as_ref()
            .and_then(|(_, _, old)| old.executable.clone())
            .filter(|path| Path::new(path).is_file())
            .or_else(|| ranked.first().cloned());
        entries.push(Entry {
            id: key(
                "entry",
                &format!("{action}:{game_id}:{}", candidate.install_path),
            ),
            action: action.into(),
            selected: true,
            game_id,
            installation_id,
            name,
            library: candidate.client.clone(),
            install_path: candidate.install_path.clone(),
            previous_path,
            executable_candidates: ranked,
            executable,
            update_kind: Some(kind.into()),
            integration,
            image: None,
            message: None,
            sources,
            artwork: observations
                .iter()
                .find_map(|candidate| candidate.artwork.clone()),
        });
    }
    for (game_id, game) in &saved.games {
        for (installation_id, installation) in &game.installations {
            if replaced.contains(&(game_id.clone(), installation_id.clone()))
                || !absent(&installation.root)
                || !installation
                    .sources
                    .iter()
                    .all(|source| successful.contains(&source.client))
            {
                continue;
            }
            entries.push(Entry {
                id: key("entry", &format!("remove:{game_id}:{installation_id}")),
                action: "remove".into(),
                selected: false,
                game_id: game_id.clone(),
                installation_id: installation_id.clone(),
                name: game.name.clone(),
                library: installation
                    .sources
                    .first()
                    .map(|source| source.client.clone())
                    .unwrap_or_else(|| "disk".into()),
                install_path: installation.root.clone(),
                previous_path: None,
                executable_candidates: vec![],
                executable: None,
                update_kind: None,
                integration: game.integration.clone(),
                image: None,
                message: Some("This installation is no longer present on disk.".into()),
                sources: vec![],
                artwork: None,
            });
        }
    }
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    entries
}
