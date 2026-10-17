//! Profile-owned use of the existing isolated feeder and output journal.
use osg_mapping::{
    profile::{Axis, Layout},
    program::VirtualController,
};
use osg_virtual_controller::vjoy::{Binding, Slot, Status, AXIS_USAGES};
use std::collections::BTreeSet;

#[derive(Clone)]
pub(crate) struct PreparedHandoff {
    pub source: crate::profile_session::HandoffSource,
    pub slots: Vec<Slot>,
    /// Starts before inspection I/O, so slow queries cannot extend freshness.
    pub observed: std::time::Instant,
}
impl PreparedHandoff {
    pub(crate) fn validate(
        &self,
        previous: Option<&crate::profile_session::OwnedRun>,
        current: Option<&osg_virtual_controller::session::RunningOutput>,
        bindings: &[Binding],
        epoch: u64,
        authority: Option<&crate::profile_session::RunOwner>,
    ) -> Result<(), String> {
        if self.observed.elapsed().as_micros()
            >= u128::from(osg_virtual_controller::feeder::LEASE_US)
        {
            return Err("The retiring virtual-slot inspection expired before takeover.".into());
        }
        validate_handoff(
            &self.source,
            previous,
            current,
            bindings,
            &self.slots,
            epoch,
            authority,
        )
    }
}
pub(crate) struct Selection {
    pub bindings: Vec<Binding>,
    pub reused: bool,
}
fn owned_slot(proof: &osg_virtual_controller::session::RunningOutput, slot: &Slot) -> bool {
    slot.status == Status::Busy
        && slot.owner_pid == Some(proof.process_id())
        && proof
            .bindings()
            .iter()
            .any(|old| old.slot == slot.id && old.match_slot(slot).is_ok())
}
pub(crate) fn select(
    controllers: &[VirtualController],
    slots: &[Slot],
    outgoing: Option<&osg_virtual_controller::session::RunningOutput>,
) -> Result<Selection, String> {
    let mut used = BTreeSet::new();
    let mut result = vec![];
    let mut reused = false;
    for controller in controllers {
        let layout = Layout {
            buttons: u16::try_from(controller.layout.buttons.len())
                .map_err(|_| "Virtual button capacity exceeded.")?,
            axes: controller
                .layout
                .axes
                .iter()
                .map(|axis| Axis {
                    range: axis.range,
                    fallback: axis.fallback,
                })
                .collect(),
            hats: u8::try_from(controller.layout.hats.len())
                .map_err(|_| "Virtual hat capacity exceeded.")?,
        };
        layout.validate()?;
        let mut available = slots.iter().collect::<Vec<_>>();
        // Preserve a compatible existing logical target's native slot before
        // considering another eligible slot. No driver status is normalized.
        available.sort_by_key(|slot| {
            (
                if outgoing.is_some_and(|proof| {
                    proof
                        .bindings()
                        .iter()
                        .any(|old| old.slot == slot.id && old.target == controller.id)
                }) {
                    0
                } else {
                    1
                },
                slot.id,
            )
        });
        let binding = available.into_iter().filter(|slot| !used.contains(&slot.id))
            .filter(|slot| slot.status == Status::Free || outgoing.is_some_and(|proof| owned_slot(proof, slot)))
            .find_map(|slot| {
                let axes = AXIS_USAGES.iter().copied().filter(|usage|slot.axes.iter().any(|axis|axis.usage==*usage)).collect();
                let binding = Binding { target: controller.id.clone(), slot: slot.id, axes, layout: layout.clone() };
                binding.match_slot(slot).is_ok().then_some((binding, slot.status == Status::Busy))
            }).ok_or_else(||format!("No free or exactly owned retiring backend slot matches the complete layout of {}. Configure an unused matching slot before Apply.",controller.id))?;
        used.insert(binding.0.slot);
        reused |= binding.1;
        result.push(binding.0);
    }
    Ok(Selection {
        bindings: result,
        reused,
    })
}
/// Shared by native capture and injected Host tests. The proof is volatile and
/// must be rederived from the exact still-live outgoing Session at this fence.
pub(crate) fn validate_handoff(
    source: &crate::profile_session::HandoffSource,
    previous: Option<&crate::profile_session::OwnedRun>,
    current: Option<&osg_virtual_controller::session::RunningOutput>,
    bindings: &[Binding],
    slots: &[Slot],
    epoch: u64,
    authority: Option<&crate::profile_session::RunOwner>,
) -> Result<(), String> {
    let proof = source
        .output
        .as_ref()
        .ok_or("The retiring output proof is missing.")?;
    let previous = previous.ok_or("The retiring virtual output has no previous run.")?;
    let previous_matches = serde_json::to_vec(previous).map_err(|e| e.to_string())?
        == serde_json::to_vec(&source.run).map_err(|e| e.to_string())?;
    if source.admission_epoch != epoch
        || authority != Some(&source.run.owner)
        || !previous_matches
        || proof.token().session != source.run.owner.session
        || proof.token().generation.0 != source.run.owner.generation
        || current.is_none_or(|current| !proof.same_as(current))
    {
        return Err(
            "The exact retiring output owner or live worker changed before takeover.".into(),
        );
    }
    let mut reused = false;
    for binding in bindings {
        let slot = slots
            .iter()
            .find(|slot| slot.id == binding.slot)
            .ok_or("The selected virtual slot disappeared before takeover.")?;
        binding.match_slot(slot)?;
        if proof.bindings().iter().any(|old| old.slot == slot.id) {
            if !owned_slot(proof, slot) {
                return Err(
                    "The retiring virtual slot no longer has its exact worker owner and layout."
                        .into(),
                );
            }
            reused = true;
        } else if slot.status != Status::Free {
            return Err("A selected virtual slot became owned before takeover.".into());
        }
    }
    if !reused {
        return Err("No selected virtual slot belongs to the retiring worker.".into());
    }
    Ok(())
}
/// Repeated native neutralization must not poll an already stopped Session.
pub(crate) fn neutralize_output(
    output: &mut osg_virtual_controller::session::Session,
    recording: &osg_mapping::runtime::RecordingSink,
) -> Result<(), String> {
    if !output.active() {
        return Ok(());
    }
    let flush = output.poll(recording);
    let stop = output.stop();
    flush.and(stop)
}
/// Explicit recovery uses the retained journal lock before any cold-store open.
/// It cannot restart reports or use another run's captured output.
pub(crate) fn restore_retained_output(
    output: &mut osg_virtual_controller::session::Session,
    owner: &crate::profile_session::RunOwner,
    recovery: bool,
) -> Result<(), String> {
    if output.token().session != owner.session || output.token().generation.0 != owner.generation {
        return Err("The retained virtual output belongs to another native run.".into());
    }
    if output.active() {
        return Err("Stop the retained virtual output before restoring it.".into());
    }
    if output.recovery_required() {
        if !recovery {
            return Err("The retained virtual output requires explicit recovery.".into());
        }
        output.recover()?;
    }
    Ok(())
}
pub(crate) fn journal_name(owner: &crate::profile_session::RunOwner) -> Result<String, String> {
    if uuid::Uuid::parse_str(&owner.session)
        .ok()
        .map(|id| id.to_string())
        .as_deref()
        != Some(&owner.session)
        || owner.generation == 0
    {
        return Err("Invalid native output recovery owner.".into());
    }
    Ok(format!(
        "profile-output-{}-{}.json",
        owner.session, owner.generation
    ))
}
pub(crate) fn recover(owner: &crate::profile_session::RunOwner) -> Result<(), String> {
    let path = crate::config_paths::config_path().with_file_name(journal_name(owner)?);
    recover_output(osg_virtual_controller::entry::factory(), path)
}
pub(crate) fn recover_output(
    factory: osg_virtual_controller::session::Factory,
    path: std::path::PathBuf,
) -> Result<(), String> {
    let Some(record) = osg_virtual_controller::output_journal::inspect(&path)? else {
        return Ok(());
    };
    if record.phase == osg_virtual_controller::output_journal::Phase::Clean {
        return Ok(());
    }
    // Do not retain a Store lock while recover_stored_exact takes that same
    // lock. Its exact comparison fences edits after this passive inspection.
    osg_virtual_controller::session::Session::recover_stored_exact(factory, path, &record)
}
#[cfg(test)]
pub(crate) fn bindings(
    controllers: &[VirtualController],
    slots: &[Slot],
) -> Result<Vec<Binding>, String> {
    Ok(select(controllers, slots, None)?.bindings)
}
#[cfg(test)]
mod tests {
    use super::*;
    use osg_mapping::{
        profile::AxisRange,
        program::{ControllerAxis, ControllerLayout},
    };
    fn controller(id: &str) -> VirtualController {
        VirtualController {
            id: id.into(),
            layout: ControllerLayout {
                buttons: vec!["button".into()],
                axes: vec![ControllerAxis {
                    port_id: "axis".into(),
                    range: AxisRange::Bipolar,
                    fallback: 0,
                }],
                hats: vec![],
            },
        }
    }
    fn slot(id: u8, status: Status) -> Slot {
        Slot {
            id,
            status,
            owner_pid: None,
            buttons: 1,
            axes: vec![osg_virtual_controller::vjoy::Axis {
                usage: 0x30,
                min: 0,
                max: 32767,
            }],
            discrete_hats: 0,
            continuous_hats: 0,
            force_feedback: false,
        }
    }
    #[test]
    fn logical_targets_get_distinct_exact_slots_deterministically() {
        let planned = bindings(
            &[controller("group-a"), controller("group-b")],
            &[slot(4, Status::Free), slot(2, Status::Free)],
        )
        .unwrap();
        assert_eq!(
            planned
                .iter()
                .map(|binding| (binding.target.as_str(), binding.slot))
                .collect::<Vec<_>>(),
            vec![("group-a", 2), ("group-b", 4)]
        );
        assert_eq!(planned[0].axes, vec![0x30]);
        assert_eq!(planned[0].fallback().axes, vec![0]);
    }
    #[test]
    fn busy_missing_or_mismatched_layouts_are_preserved() {
        for status in [
            Status::Busy,
            Status::Owned,
            Status::Missing,
            Status::Unknown,
        ] {
            assert!(bindings(&[controller("one")], &[slot(1, status)]).is_err());
        }
        let mut incompatible = slot(1, Status::Free);
        incompatible.buttons = 2;
        assert!(bindings(&[controller("one")], &[incompatible]).is_err());
        let mut incompatible = slot(1, Status::Free);
        incompatible.force_feedback = true;
        assert!(bindings(&[controller("one")], &[incompatible]).is_err());
        assert!(bindings(
            &[controller("one"), controller("two")],
            &[slot(1, Status::Free)]
        )
        .is_err());
        assert!(bindings(&[], &[]).unwrap().is_empty());
    }
    #[test]
    fn journal_reference_is_derived_only_from_native_owner() {
        let mut owner = crate::profile_session::RunOwner {
            session: uuid::Uuid::new_v4().to_string(),
            generation: 1,
            profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(),
            profile_revision: 1,
            game_id: None,
        };
        assert!(journal_name(&owner).unwrap().ends_with("-1.json"));
        owner.session = "../outside".into();
        assert!(journal_name(&owner).is_err());
        owner.session = uuid::Uuid::new_v4().to_string();
        owner.generation = 0;
        assert!(journal_name(&owner).is_err());
    }
}
