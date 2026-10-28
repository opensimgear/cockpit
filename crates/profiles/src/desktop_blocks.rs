use crate::DesktopOutput;
use osg_mapping::desktop::Action;

/// Keyboard and Mouse are separate graph blocks using one native scoped adapter.
pub fn desktop_outputs_valid(outputs: &[DesktopOutput]) -> bool {
    if outputs.len() > 2 { return false; }
    let mut devices = 0;
    for output in outputs {
        if output.scope != outputs[0].scope { return false; }
        let mut block = 0;
        for definition in &output.actions {
            block |= match definition.action {
                Action::MouseHold { .. } | Action::MouseClick { .. } | Action::Wheel { .. } | Action::Pointer { .. } => 2,
                _ => 1,
            };
        }
        if devices & block != 0 { return false; }
        devices |= block;
    }
    true
}
