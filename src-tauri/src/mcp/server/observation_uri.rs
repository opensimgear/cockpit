//! Strict URI admission for MCP observation subscriptions.

use crate::persistence::ui::OBSERVED_KEYS;
use serde_json::{json, Value};

const PREFIX: &str = "cockpit://observations/";
const FIXED_TOPICS: [(&str, &str); 22] = [
    ("activity.log", "Activity log revision"),
    ("settings.activity", "Committed activity retention settings"),
    ("profiles.library", "Saved profile library"),
    ("profiles.runtime", "Applied profile runtime"),
    ("profiles.desktop", "Keyboard and mouse output readiness"),
    ("profiles.activation", "Profile activation progress"),
    ("profiles.learning", "Physical input learning revision"),
    ("profiles.debug-inputs", "Applied debug controller input revision"),
    ("profiles.retired-recovery", "Retained output recovery revision"),
    ("profiles.resources", "Profile resource inventory revision"),
    ("workspace.state", "Gateway workspace state"),
    ("games.state", "Configured games"),
    ("games.jobs", "Game discovery and artwork jobs"),
    ("games.telemetry", "Game telemetry revision"),
    ("companion.listener", "Companion listener settings revision"),
    ("companion.presence", "Companion presence and pairing revision"),
    ("companion.firmware", "Companion USB firmware update progress"),
    ("controllers.inventory", "Host controller inventory"),
    ("usb.inventory", "USB module inventory revision"),
    ("system.inventory", "Supplemental system inventory revision"),
    ("settings.desktop", "Committed keyboard and mouse output settings"),
    ("storage.backups", "Backup inventory and operation phase"),
];

pub(super) fn resources(broker: osg_streams::Broker) -> impl Iterator<Item = Value> {
    let preferences = OBSERVED_KEYS.into_iter().map(|key| {
        (format!("preferences.{key}"), format!("UI preference: {key}"))
    });
    FIXED_TOPICS.into_iter()
        .map(|(topic, title)| (topic.to_owned(), title.to_owned()))
        .chain(preferences)
        // The controller cache provides an on-demand atomic baseline even
        // before its first observer. Other fixed topics register at startup.
        .filter(move |(topic, _)| topic == "controllers.inventory" || broker.available_to(topic, "mcp"))
        .map(|(topic, title)| {
            json!({"uri":format!("{PREFIX}{topic}"),"name":topic,"title":title,"mimeType":"application/json"})
        })
}

/// Return the native observation topic named by an explicitly supported URI.
///
/// MCP clients can subscribe only to read-only projections that are safe to
/// expose. Dynamic topic suffixes are validated before the URI is admitted.
pub(super) fn topic(uri: &str) -> Option<&str> {
    let topic = uri.strip_prefix(PREFIX)?;
    if topic.is_empty()
        || topic.bytes().any(|byte| {
            matches!(byte, b'/' | b'?' | b'#' | b'%')
                || byte.is_ascii_whitespace()
                || byte.is_ascii_control()
        })
    {
        return None;
    }

    let allowed = FIXED_TOPICS.iter().any(|(name, _)| *name == topic)
        || topic.strip_prefix("preferences.").is_some_and(|key| OBSERVED_KEYS.contains(&key))
        || topic
            .strip_prefix("games.detail.")
            .is_some_and(osg_profiles::activation::valid_game_id)
        || topic
            .strip_prefix("profiles.samples.")
            .is_some_and(valid_sha256)
        || topic
            .strip_prefix("controllers.input.")
            .is_some_and(valid_uuid)
        || topic.strip_prefix("usb.inputs.").is_some_and(valid_sha256)
        || topic
            .strip_prefix("usb.metadata.")
            .is_some_and(valid_sha256)
        || valid_can_topic(topic);

    allowed.then_some(topic)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && lower_hex(value)
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn valid_can_topic(topic: &str) -> bool {
    let Some((gateway, module)) = topic.strip_prefix("can.inputs.")
        .and_then(|suffix| suffix.split_once('.')) else { return false };
    gateway.len() == 24 && lower_hex(gateway) && module.len() == 32 && lower_hex(module)
}

fn lower_hex(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::topic;

    #[test]
    fn admits_only_supported_observation_uris_and_valid_dynamic_suffixes() {
        for expected in [
            "profiles.library",
            "profiles.runtime",
            "profiles.activation",
            "profiles.learning",
            "profiles.debug-inputs",
            "profiles.retired-recovery",
            "profiles.resources",
            "workspace.state",
            "games.state",
            "games.jobs",
            "games.telemetry",
            "games.detail.dcs",
            "controllers.inventory",
            "usb.inventory",
            "system.inventory",
            "preferences.remembered",
            "controllers.input.11111111-1111-4111-8111-111111111111",
            "can.inputs.0123456789abcdef01234567.0123456789abcdef0123456789abcdef",
        ] {
            assert_eq!(topic(&format!("cockpit://observations/{expected}")), Some(expected));
        }
        let sha = "a".repeat(64);
        for name in ["usb.inputs", "usb.metadata", "profiles.samples"] {
            let expected = format!("{name}.{sha}");
            assert_eq!(topic(&format!("cockpit://observations/{expected}")), Some(expected.as_str()));
        }
    }

    #[test]
    fn rejects_noncanonical_uris_unknown_topics_and_malformed_suffixes() {
        for uri in [
            "cockpit://observations/",
            "cockpit://observations/games",
            "cockpit://observations/preferences.windows",
            "cockpit://observations/preferences.remembered.extra",
            "cockpit://observations/settings.mcp",
            "cockpit://observations/games.detail.",
            "cockpit://observations/games.detail.bad/id",
            "cockpit://observations/controllers.input.not-a-uuid",
            "cockpit://observations/can.inputs.0123456789abcdef01234567.bad",
            "cockpit://observations/usb.inputs.NOT-HEX",
            "cockpit://observations/profiles.runtime?x=1",
            "cockpit://observations/profiles.runtime#fragment",
            "cockpit://observations/profiles.runtime/extra",
            "cockpit://observations/%70rofiles.runtime",
            "cockpit://observations.evil/profiles.runtime",
            "Cockpit://observations/profiles.runtime",
            "https://observations/profiles.runtime",
        ] {
            assert_eq!(topic(uri), None, "unexpectedly admitted {uri}");
        }
    }
}
