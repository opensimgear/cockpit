use super::*;

/// A platform owns its installation; disk discovery supplements platform libraries.
pub(super) fn priority(candidate: &Candidate) -> (bool, bool) {
    (candidate.client == "disk", candidate.game_id.is_none())
}

pub(super) fn path_key(path: &str) -> String {
    crate::game_discovery::normalized_path(path)
        .trim_end_matches('\\')
        .to_owned()
}

pub(super) fn publish(review: &mut Review, candidates: &[Candidate]) {
    let mut ordered = candidates
        .iter()
        .filter(|candidate| candidate.kind != "tool")
        .collect::<Vec<_>>();
    ordered.sort_by_key(|candidate| priority(candidate));
    let mut roots = BTreeSet::new();
    let observations = ordered
        .into_iter()
        .filter(|candidate| roots.insert(path_key(&candidate.install_path)))
        .cloned()
        .collect::<Vec<_>>();
    for library in &mut review.libraries {
        library.found = observations
            .iter()
            .filter(|candidate| candidate.client == library.id)
            .count();
    }
    review.observations = observations;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(client: &str, root: &str, recognized: bool) -> Candidate {
        Candidate {
            id: format!("{client}:{root}"),
            client: client.into(),
            name: "Same title".into(),
            install_path: root.into(),
            executable_candidates: vec![],
            preferred_executable: None,
            game_id: recognized.then(|| "dcs".into()),
            kind: "game".into(),
            app_id: "42".into(),
            launcher: None,
            artwork: None,
        }
    }

    fn review() -> Review {
        Review {
            libraries: ["steam", "epic", "disk"]
                .into_iter()
                .map(|id| Library {
                    id: id.into(),
                    name: id.into(),
                    status: "preparing".into(),
                    found: 99,
                    message: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn platform_owned_installations_are_not_repeated_in_local_paths() {
        let mut review = review();
        publish(
            &mut review,
            &[
                candidate("disk", r"c:\games\same\", true),
                candidate("steam", r"\\?\C:\Games\Same", false),
                candidate("epic", r"C:\Games\Other", false),
                candidate("disk", r"C:\Games\Other", true),
            ],
        );
        assert_eq!(
            review
                .observations
                .iter()
                .map(|entry| entry.client.as_str())
                .collect::<Vec<_>>(),
            vec!["steam", "epic"]
        );
        assert_eq!(
            review
                .libraries
                .iter()
                .map(|library| library.found)
                .collect::<Vec<_>>(),
            vec![1, 1, 0]
        );
    }

    #[test]
    fn separate_copies_of_the_same_title_remain_separate_installations() {
        let mut review = review();
        publish(
            &mut review,
            &[
                candidate("steam", r"C:\Steam\DCS", true),
                candidate("disk", r"D:\Standalone\DCS", true),
                candidate("disk", r"d:\standalone\dcs\", true),
            ],
        );
        assert_eq!(review.observations.len(), 2);
        assert_eq!(review.libraries[0].found, 1);
        assert_eq!(review.libraries[2].found, 1);
        publish(&mut review, &[]);
        assert!(review.observations.is_empty());
        assert!(review.libraries.iter().all(|library| library.found == 0));
    }

    #[test]
    fn platform_grouping_preserves_recognition_and_all_source_evidence() {
        let directory = crate::test_directory();
        let root = directory.path().to_str().unwrap();
        let entries = super::super::reconcile::reconcile(
            &crate::game_installations::Settings::default(),
            vec![
                candidate("disk", root, true),
                candidate("steam", root, false),
            ],
            &BTreeSet::from(["steam".into(), "disk".into()]),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].library, "steam");
        assert_eq!(entries[0].game_id, "dcs");
        assert_eq!(entries[0].integration.as_deref(), Some("dcs"));
        assert_eq!(entries[0].sources.len(), 2);
    }
}
