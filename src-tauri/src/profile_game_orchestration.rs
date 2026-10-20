//! Pre-launch selection is an intent over native installation identities. It
//! cannot manufacture a process session or authorize a simulator context.
use super::*;
use osg_profiles::activation::GameIdentity;
#[path = "profile_game_orchestration_runtime.rs"]
mod runtime;

#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchIntent {
    profile_id: String,
    game_id: Option<String>,
}
fn launch_intent(
    settings: &Settings,
    requested: &GameIdentity,
    running: &[VerifiedGame],
    profiles: &[osg_profiles::ProfileDocument],
    active_profile_id: Option<&str>,
) -> Result<LaunchIntent, String> {
    settings.validate()?;
    requested.validate()?;
    if running.len() > osg_profiles::activation::MAX_RULES {
        return Err("The native game observation exceeds its bound.".into());
    }
    let mut observed_ids = std::collections::BTreeSet::new();
    for observed in running {
        observed.identity.validate()?;
        if observed.process_session.is_empty()
            || observed.process_session.len() > 256
            || observed.process_session.chars().any(char::is_control)
            || !observed_ids.insert(&observed.identity.game_id)
        {
            return Err(
                "Native launch selection needs distinct verified game process sessions.".into(),
            );
        }
    }
    let intent = if settings.mode == SelectionMode::Manual {
        LaunchIntent {
            profile_id: active_profile_id.unwrap_or(osg_profiles::DEFAULT_PROFILE_ID).into(),
            game_id: Some(requested.game_id.clone()),
        }
    } else {
        let mut matches = settings
            .rules
            .iter()
            .filter(|rule| {
                rule.enabled
                    && (rule.identity == *requested
                        || running
                            .iter()
                            .any(|observed| observed.identity == rule.identity))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(a.identity.game_id.cmp(&b.identity.game_id))
        });
        match matches.first() {
            None => LaunchIntent {
                profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(),
                game_id: None,
            },
            Some(first) => {
                let ties = matches
                    .iter()
                    .take_while(|rule| rule.priority == first.priority)
                    .map(|rule| rule.identity.game_id.as_str())
                    .collect::<Vec<_>>();
                if ties.len() != 1 {
                    return Err(format!("Equal highest-priority game rules conflict: {}. Choose a unique priority before launching.",ties.join(", ")));
                }
                LaunchIntent {
                    profile_id: first.profile_id.clone(),
                    game_id: Some(first.identity.game_id.clone()),
                }
            }
        }
    };
    let document = profiles
        .iter()
        .find(|profile| profile.id == intent.profile_id)
        .ok_or("The selected launch rule references a removed profile.")?;
    if intent.game_id.as_ref().is_some_and(|game| {
        !document.eligibility.game_ids.is_empty() && !document.eligibility.game_ids.contains(game)
    }) {
        return Err(
            "The selected saved profile is not eligible for its effective game context.".into(),
        );
    }
    Ok(intent)
}

#[cfg(test)]
#[path = "profile_game_orchestration_tests.rs"]
mod tests;
