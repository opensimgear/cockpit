//! Deterministic suggestions for choosing a game's main executable.
//!
//! This ranks executable paths only; it never uses a fuzzy title match to
//! merge game identities.

use std::cmp::Reverse;

/// Put an evidenced executable first, then rank the remaining candidates by
/// how closely their filename resembles the game title.
pub(crate) fn rank(title: &str, candidates: &[String], evidenced: Option<&str>) -> Vec<String> {
    let mut ranked = candidates.to_vec();
    ranked.sort_by(|left, right| {
        let left_evidenced = evidenced.is_some_and(|path| same_path(path, left));
        let right_evidenced = evidenced.is_some_and(|path| same_path(path, right));
        right_evidenced
            .cmp(&left_evidenced)
            .then_with(|| Reverse(similarity(title, left)).cmp(&Reverse(similarity(title, right))))
            .then_with(|| normalized_path(left).cmp(&normalized_path(right)))
            .then_with(|| left.cmp(right))
    });
    ranked.dedup_by(|left, right| same_path(left, right));
    ranked
}

fn same_path(left: &str, right: &str) -> bool {
    left.replace('\\', "/").to_lowercase() == right.replace('\\', "/").to_lowercase()
}

fn normalized_path(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_lowercase()
}

fn similarity(title: &str, path: &str) -> i32 {
    let normalized = path.replace('\\', "/");
    let filename = normalized.rsplit('/').next().unwrap_or(path);
    let stem = filename
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(filename);
    let title_tokens = title_tokens(title);
    let raw_file_tokens = tokens(stem);
    let file_tokens = runtime_tokens(stem);
    let build_suffix_count = raw_file_tokens.len().saturating_sub(file_tokens.len());
    if title_tokens.is_empty() || file_tokens.is_empty() {
        return -penalty(stem) - path_penalty(path);
    }

    let title_joined = title_tokens.concat();
    let file_joined = file_tokens.concat();
    let acronym = title_tokens
        .iter()
        .map(|token| {
            if token.chars().next().is_some_and(char::is_numeric) {
                token.as_str()
            } else {
                token
                    .get(..token.chars().next().map_or(0, char::len_utf8))
                    .unwrap_or(token)
            }
        })
        .collect::<String>();
    let full_first_acronym = title_tokens.first().map(|first| {
        title_tokens
            .iter()
            .skip(1)
            .filter(|token| token.chars().next().is_some_and(char::is_numeric))
            .fold(first.clone(), |mut value, token| {
                value.push_str(token);
                value
            })
    });
    let mut score = 0;
    if title_joined == file_joined {
        score = 1200;
    } else if acronym == file_joined || full_first_acronym.as_deref() == Some(file_joined.as_str())
    {
        score = 1150;
    } else if [Some(acronym.as_str()), full_first_acronym.as_deref()]
        .into_iter()
        .flatten()
        .any(|prefix| {
            prefix.len() < file_joined.len()
                && file_joined.starts_with(prefix)
                && file_joined[prefix.len()..].chars().all(char::is_numeric)
        })
    {
        score = 1125;
    } else if compact_sequel_match(&title_joined, &file_joined) {
        score = 1080;
    } else if file_joined.contains(&title_joined) || title_joined.contains(&file_joined) {
        score = 900;
    }

    let token_hits = title_tokens
        .iter()
        .filter(|token| file_tokens.iter().any(|candidate| candidate == *token))
        .count();
    if token_hits > 0 {
        score = score.max((token_hits * 800 / title_tokens.len()) as i32);
    }
    score = score.max(edit_similarity(&title_joined, &file_joined));
    score - penalty(stem) - path_penalty(path) - (build_suffix_count as i32 * 20)
}

fn title_tokens(title: &str) -> Vec<String> {
    let mut tokens = tokens(title)
        .into_iter()
        .map(|token| roman_numeral(&token).unwrap_or(token))
        .collect::<Vec<_>>();
    while tokens.last().is_some_and(|token| {
        matches!(
            token.as_str(),
            "demo"
                | "edition"
                | "deluxe"
                | "ultimate"
                | "infernal"
                | "definitive"
                | "special"
                | "standard"
                | "premium"
                | "complete"
                | "gold"
                | "platinum"
                | "collector"
                | "collectors"
                | "anniversary"
                | "trial"
                | "beta"
                | "alpha"
        )
    }) {
        tokens.pop();
    }
    tokens
}

fn roman_numeral(token: &str) -> Option<String> {
    Some(
        match token {
            "i" => "1",
            "ii" => "2",
            "iii" => "3",
            "iv" => "4",
            "v" => "5",
            "vi" => "6",
            "vii" => "7",
            "viii" => "8",
            "ix" => "9",
            "x" => "10",
            _ => return None,
        }
        .into(),
    )
}

fn compact_sequel_match(title: &str, candidate: &str) -> bool {
    let title_chars = title.chars().collect::<Vec<_>>();
    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    if !(3..=8).contains(&candidate_chars.len())
        || !title_chars.iter().any(|character| character.is_numeric())
        || candidate_chars.first() != title_chars.first()
    {
        return false;
    }
    let title_digits = title_chars
        .iter()
        .filter(|character| character.is_numeric())
        .collect::<Vec<_>>();
    let candidate_digits = candidate_chars
        .iter()
        .filter(|character| character.is_numeric())
        .collect::<Vec<_>>();
    if title_digits != candidate_digits {
        return false;
    }
    let mut position = 0;
    candidate_chars.iter().all(|character| {
        let Some(offset) = title_chars[position..]
            .iter()
            .position(|title_character| title_character == character)
        else {
            return false;
        };
        position += offset + 1;
        true
    })
}

fn path_penalty(path: &str) -> i32 {
    let components = path
        .replace('\\', "/")
        .split('/')
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let third_party = components.iter().any(|component| {
        matches!(
            component.as_str(),
            "thirdparty" | "third_party" | "sdk" | "support"
        )
    });
    let engine = components.iter().any(|component| component == "engine");
    (if third_party { 1400 } else { 0 }) + (if engine { 200 } else { 0 })
}

fn runtime_tokens(value: &str) -> Vec<String> {
    let mut tokens = tokens(value);
    while tokens.last().is_some_and(|token| {
        matches!(
            token.as_str(),
            "steam" | "win32" | "win64" | "shipping" | "development" | "test" | "debug" | "client"
        )
    }) {
        tokens.pop();
    }
    tokens
}

fn tokens(value: &str) -> Vec<String> {
    let chars = value.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut current = String::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        let camel_boundary = ch.is_uppercase()
            && !current.is_empty()
            && chars
                .get(index.wrapping_sub(1))
                .is_some_and(|previous| previous.is_lowercase() || previous.is_numeric());
        if camel_boundary {
            tokens.push(std::mem::take(&mut current));
        }
        current.extend(ch.to_lowercase());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn edit_similarity(left: &str, right: &str) -> i32 {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (index, a) in left.iter().enumerate() {
        current[0] = index + 1;
        for (offset, b) in right.iter().enumerate() {
            current[offset + 1] = (previous[offset + 1] + 1)
                .min(current[offset] + 1)
                .min(previous[offset] + usize::from(a != b));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let max_len = left.len().max(right.len());
    ((max_len - previous[right.len()]) * 800 / max_len) as i32
}

fn penalty(stem: &str) -> i32 {
    let normalized = stem.to_lowercase();
    [
        "launcher",
        "bootstrap",
        "webhelper",
        "browser",
        "cef",
        "helper",
        "server",
        "dedicated",
        "error",
        "support",
        "service",
        "editor",
        "benchmark",
        "config",
        "configuration",
        "diagnostic",
        "diagnostics",
        "diag",
        "crash",
        "report",
        "unins",
        "uninstall",
        "setup",
        "install",
        "updater",
        "update",
        "redist",
        "prereq",
        "redistributable",
    ]
    .iter()
    .any(|word| normalized.contains(word))
    .then_some(2000)
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{path_penalty, rank};

    fn paths(names: &[&str]) -> Vec<String> {
        names
            .iter()
            .map(|name| format!(r"C:\Games\{name}"))
            .collect()
    }

    fn first(title: &str, names: &[&str]) -> String {
        rank(title, &paths(names), None)[0].clone()
    }

    #[test]
    fn exact_and_unicode_camel_names_rank_first() {
        assert!(
            first("Cyberpunk 2077", &["Cyberpunk2077.exe", "Launcher.exe"])
                .ends_with("Cyberpunk2077.exe")
        );
        assert!(first("ÜberRacer", &["Uber-Racer.exe", "Other.exe"]).ends_with("Uber-Racer.exe"));
    }

    #[test]
    fn initials_and_sequel_digits_rank_first() {
        assert!(first("Euro Truck Simulator 2", &["ETS2.exe", "Game.exe"]).ends_with("ETS2.exe"));
        assert!(first("Euro Truck Simulator 20", &["ETS20.exe", "Game.exe"]).ends_with("ETS20.exe"));
        assert!(first("FIFA 23", &["FIFA23.exe", "Game.exe"]).ends_with("FIFA23.exe"));
        assert!(
            first("Assetto Corsa Competizione", &["ACC.exe", "Launcher.exe"]).ends_with("ACC.exe")
        );
    }

    #[test]
    fn close_typo_beats_unrelated_filename() {
        assert!(
            first("Hollow Knight", &["HollowKnigt.exe", "Unrelated.exe"])
                .ends_with("HollowKnigt.exe")
        );
    }

    #[test]
    fn helper_penalty_overrides_a_title_match() {
        assert!(first(
            "Example Game",
            &["ExampleGameLauncher.exe", "ExampleGame.exe"]
        )
        .ends_with("ExampleGame.exe"));
        assert!(first(
            "Example Game",
            &["ExampleGameServer.exe", "ExampleGame.exe"]
        )
        .ends_with("ExampleGame.exe"));
        for helper in [
            "ExampleGameEditor.exe",
            "ExampleGameBenchmark.exe",
            "ExampleGameConfiguration.exe",
            "ExampleGameDiagnostics.exe",
            "ExampleGameBrowser.exe",
            "ExampleGameError.exe",
            "ExampleGameCef.exe",
            "ExampleGameSupport.exe",
            "ExampleGameService.exe",
        ] {
            assert!(
                first("Example Game", &[helper, "ExampleGame.exe"]).ends_with("ExampleGame.exe")
            );
        }
    }

    #[test]
    fn unreal_shipping_suffix_does_not_outscore_the_plain_runtime() {
        assert!(first(
            "Example Game",
            &["ExampleGame-Win64-Shipping.exe", "ExampleGame.exe"]
        )
        .ends_with("ExampleGame.exe"));
    }

    #[test]
    fn real_library_helpers_lose_to_the_game_runtime() {
        assert!(first("Arma Reforger", &["ArmaReforger_BE.exe", "ArmaReforgerSteam.exe"]).ends_with("ArmaReforgerSteam.exe"));
        let arc = rank(
            "ARC Raiders",
            &[
                r"C:\Games\ARC Raiders\PioneerGame.exe".into(),
                r"C:\Games\ARC Raiders\Engine\Binaries\Win64\EpicWebHelper.exe".into(),
            ],
            None,
        );
        assert!(arc[0].ends_with("PioneerGame.exe"));

        let diablo = rank(
            "Diablo II: Resurrected – Infernal Edition",
            &[
                r"C:\Games\Diablo\BlizzardError.exe".into(),
                r"C:\Games\Diablo\D2R.exe".into(),
            ],
            None,
        );
        assert!(diablo[0].ends_with("D2R.exe"));
    }

    #[test]
    fn sequel_abbreviations_and_engine_directories_rank_sensibly() {
        assert!(first("Battlefield 6", &["Unrelated.exe", "BF6.exe"]).ends_with("BF6.exe"));
        let ranked = rank(
            "Example Game",
            &[
                r"C:\Games\Engine\Binaries\Win64\ExampleGameHelper.exe".into(),
                r"C:\Games\ExampleGame.exe".into(),
            ],
            None,
        );
        assert!(ranked[0].ends_with("ExampleGame.exe"));
        assert_eq!(path_penalty(r"C:\Games\Binaries\Win64\ExampleGame.exe"), 0);
    }

    #[test]
    fn evidence_wins_and_remaining_ties_are_stable() {
        let candidates = paths(&["Alpha.exe", "Beta.exe"]);
        let ranked = rank("Unknown title", &candidates, Some(&candidates[1]));
        assert_eq!(ranked[0], candidates[1]);
        let tied = rank("???", &paths(&["zeta.exe", "alpha.exe"]), None);
        assert!(tied[0].ends_with("alpha.exe"));
    }

    #[test]
    fn unknown_titles_still_get_a_deterministic_suggestion() {
        let ranked = rank("???", &paths(&["zeta.exe", "alpha.exe"]), None);
        assert!(ranked[0].ends_with("alpha.exe"));
        assert!(rank("Unknown", &[], None).is_empty());
    }
}
