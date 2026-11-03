use super::*;
use inspection::samples::{CapturedSamples, NodePlan};
use std::sync::Arc;

impl<H: Host> Coordinator<H> {
    pub(crate) fn prepare_sample_plan(&self, topic: &str) -> Result<Arc<NodePlan>, String> {
        let run = self.manifest.active.as_ref().filter(|run| self.validates(&run.owner))
            .ok_or("Choose a node of the current running applied owner.")?;
        self.host.sample_plan(run, topic)
    }
    pub(crate) fn capture_samples(&self, plan: &NodePlan) -> Result<CapturedSamples, String> {
        let run = self.manifest.active.as_ref().filter(|run| run.owner == plan.owner && self.validates(&run.owner))
            .ok_or("The applied sample owner retired.")?;
        self.host.sample_capture(run, plan)
    }
}
