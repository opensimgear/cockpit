//! Allocation accounting for typed captures; no JSON on producer/route locks.
use super::*;
const MIB: usize = 1024 * 1024;
pub(super) const MAX_SOURCE_BYTES: usize = MIB;
pub(super) const MAX_AUTOMATIC_SOURCE_BYTES: usize = 3 * MIB;
pub(super) const MAX_QUEUE_BYTES: usize = 4 * MIB;
fn strings<'a>(values: impl IntoIterator<Item = &'a String>) -> usize {
    values.into_iter().map(|value| std::mem::size_of::<String>() + value.len()).sum()
}
fn summary(value: &osg_profiles::session::SnapshotSummary) -> usize {
    std::mem::size_of_val(value) + value.profile_id.len() + value.semantic_hash.len()
}
fn target(value: &Target) -> usize {
    std::mem::size_of_val(value) + value.profile_id.len()
        + value.game_id.as_ref().map_or(0, String::len) + value.process_session.as_ref().map_or(0, String::len)
}
pub(super) fn event(event: &Event) -> usize {
    std::mem::size_of::<Event>() + match event {
        Event::Library(value) => {
            value.profiles.iter().map(|row| std::mem::size_of_val(row)
                + row.id.len() + row.name.len() + row.semantic_hash.len() + strings(&row.game_ids)).sum::<usize>()
                + strings(&value.diagnostics) + value.session.selected_profile_id.len()
                + value.session.applied_snapshot.as_ref().map_or(0, summary)
                + value.session.fault.as_ref().map_or(0, String::len)
                + value.versions.iter().map(|(id, version)| 128 + id.len() + version.len()).sum::<usize>()
        },
        Event::Native(value) => {
            std::mem::size_of_val(value) + strings(&value.degradation) + value.fault.as_ref().map_or(0, String::len)
                + value.applied_snapshot.as_ref().map_or(0, summary)
                + value.owner.as_ref().map_or(0, |owner| std::mem::size_of_val(owner)
                    + owner.session.len() + owner.profile_id.len() + owner.game_id.as_ref().map_or(0, String::len))
        },
        Event::Automatic(AutomaticChange::Emergency { .. }) => 0,
        Event::Automatic(AutomaticChange::Snapshot(value)) => {
            std::mem::size_of_val(value) + value.manual_profile_id.len() + value.status.len() + value.message.len()
                + strings(&value.conflicts) + value.last_attempt.as_ref().map_or(0, target) + value.target.as_ref().map_or(0, target)
                + value.rules.iter().map(|rule| std::mem::size_of_val(rule)
                    + rule.identity.game_id.len() + rule.identity.executable.len() + rule.profile_id.len()).sum::<usize>()
                + value.preferred_profiles.iter().map(|(game, profile)| 128 + game.len() + profile.len()).sum::<usize>()
        },
    }
}
