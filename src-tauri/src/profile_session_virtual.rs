//! Profile-owned use of the existing isolated feeder and output journal.
use osg_mapping::{
    profile::{Axis, Layout},
    program::VirtualController,
};
use osg_virtual_controller::vjoy::{Binding, Slot, Status, AXIS_USAGES};
use std::collections::BTreeSet;

pub(crate) fn bindings(
    controllers: &[VirtualController],
    slots: &[Slot],
) -> Result<Vec<Binding>, String> {
    let mut used = BTreeSet::new();
    let mut result = vec![];
    let mut available = slots.iter().collect::<Vec<_>>();
    available.sort_by_key(|slot| slot.id);
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
        let binding=available.iter().filter(|slot|slot.status==Status::Free&&!used.contains(&slot.id)).find_map(|slot| {
            let axes=AXIS_USAGES.iter().copied().filter(|usage|slot.axes.iter().any(|axis|axis.usage==*usage)).collect();
            let binding=Binding{target:controller.id.clone(),slot:slot.id,axes,layout:layout.clone()};
            binding.match_slot(slot).is_ok().then_some(binding)
        }).ok_or_else(||format!("No free qualified backend slot matches the complete layout of {}. Configure an unused matching slot before Apply.",controller.id))?;
        used.insert(binding.slot);
        result.push(binding);
    }
    Ok(result)
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
    let store = osg_virtual_controller::output_journal::Store::open(path.clone())?;
    if store
        .record()
        .is_none_or(|record| record.phase == osg_virtual_controller::output_journal::Phase::Clean)
    {
        return Ok(());
    }
    osg_virtual_controller::session::Session::recover_stored(
        osg_virtual_controller::entry::factory(),
        path,
    )
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
