//! Strict URI admission for MCP observation subscriptions.

/// Return the native observation topic named by an explicitly supported URI.
///
/// MCP clients can subscribe only to read-only projections that are safe to
/// expose. Dynamic topic suffixes are validated before the URI is admitted.
pub(super) fn topic(uri: &str) -> Option<&str> {
    let topic = uri.strip_prefix("cockpit://observations/")?;
    if topic.is_empty()
        || topic.bytes().any(|byte| {
            matches!(byte, b'/' | b'?' | b'#' | b'%')
                || byte.is_ascii_whitespace()
                || byte.is_ascii_control()
        })
    {
        return None;
    }

    let allowed = matches!(topic,
        "profiles.library"
            | "profiles.runtime"
            | "profiles.activation"
            | "workspace.state"
            | "games.state"
            | "games.jobs"
            | "controllers.inventory"
    ) || topic.strip_prefix("games.detail.")
        .is_some_and(osg_profiles::activation::valid_game_id)
        || topic.strip_prefix("profiles.samples.").is_some_and(valid_sha256)
        || topic.strip_prefix("controllers.input.").is_some_and(valid_uuid)
        || topic.strip_prefix("usb.inputs.").is_some_and(valid_sha256)
        || topic.strip_prefix("usb.metadata.").is_some_and(valid_sha256)
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
            "workspace.state",
            "games.state",
            "games.jobs",
            "games.detail.dcs",
            "controllers.inventory",
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
