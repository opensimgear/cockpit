use super::*;
use osg_desktop_output::protocol::ScopeReceipt;

impl Desktop {
    pub(super) fn helper_receipt(&mut self) -> Result<ScopeReceipt, String> {
        let receipt = self.output.child_scope();
        if let Some(receipt) = receipt.filter(|receipt| receipt.live) { return Ok(receipt); }
        let error = if receipt.is_some() { "Desktop helper scope registration expired." }
            else { "Desktop helper scope registration receipt is missing." }.to_string();
        self.last_error = Some(error.clone());
        self.output.reject_child(error.clone());
        self.shadow = None;
        self.shadow_admission = None;
        self.focused = false;
        self.authorization_deadline = None;
        Err(error)
    }
    pub(super) fn poll_admissions_focused(&mut self, admissions: &[Admission], reducer: &Phase, focused: bool) -> Result<Vec<String>, String> {
        if !self.started { return Ok(vec![]); }
        let receipt = self.helper_receipt()?;
        let lost = self.shadow_admission != Some(receipt.admission);
        if lost {
            // The helper may have observed an entire away/back cycle while the
            // parent's own WinEvent snapshot stayed unchanged. Consume that
            // boundary through the same false/return path, with no admissions.
            self.poll_admissions_focused_inner(&[], reducer, false)?;
            self.shadow_admission = Some(receipt.admission);
            self.output.acknowledge_child(receipt.admission);
        }
        self.poll_admissions_focused_inner(if lost { &[] } else { admissions }, reducer, focused)
    }
}
