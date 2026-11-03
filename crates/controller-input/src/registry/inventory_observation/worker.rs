use super::{queue::{Batch, Work}, *};
use osg_streams::{Publisher, TopicSpec, MAX_CHANGE_BYTES};
use serde_json::Value;

struct Binding { broker: Broker, publisher: Option<Publisher>, previous: Option<Value>, view: Option<Arc<View>> }
impl Drop for Binding {
    fn drop(&mut self) { if let Some(publisher) = &self.publisher { self.broker.retire(publisher); } }
}
impl Binding {
    fn retire_idle(&mut self, shared: &Shared) {
        let Some(publisher) = &self.publisher else { return; };
        if !shared.begin_retire() { return; }
        if self.broker.retire_if_idle(publisher) { self.publisher = None; self.previous = None; }
        shared.finish_retire();
    }
    fn publish(&mut self, shared: &Shared, update: &Batch) -> Result<(), String> {
        if let Some(view) = &update.view { self.view = Some(view.clone()); }
        let view = self.view.as_mut().ok_or("Controller inventory metadata baseline is missing.")?;
        for row in &update.rows { Arc::make_mut(view).apply_row(row.clone())?; }
        self.retire_idle(shared);
        if self.publisher.is_none() && !shared.pinned() { return Ok(()); }
        let view = self.view.as_ref().expect("validated inventory baseline");
        #[cfg(test)]
        super::test_support::before_publication(&view.session);
        let payload = serde_json::to_value(view.as_ref()).map_err(|error| error.to_string())?;
        let clock = format!("controllers:{}", view.session);
        if let Some(publisher) = &self.publisher {
            if !update.resync && self.previous.as_ref() == Some(&payload) { return Ok(()); }
            // Reserve the same envelope allowance as the wire registry. A full
            // 256-source state fits snapshots even when it cannot fit changes.
            let oversized = serde_json::to_vec(&payload).map_err(|error| error.to_string())?.len() > MAX_CHANGE_BYTES - 2048;
            if update.resync || oversized {
                publisher.resynchronize(1, payload.clone(), update.captured_us, &clock)
            } else {
                publisher.publish_latest_state(1, payload.clone(), update.captured_us, &clock)
            }.map_err(|error| error.to_string())?;
        } else {
            self.publisher = Some(self.broker.register(TopicSpec { topic: "controllers.inventory".into(),
                source: "controller-input".into(), audiences: osg_streams::KNOWN_AUDIENCES.iter().map(|s| (*s).into()).collect() },
                payload.clone(), update.captured_us, clock).map_err(|error| error.to_string())?);
        }
        self.previous = Some(payload);
        Ok(())
    }
}
pub(super) fn run(shared: &Shared, broker: Broker) {
    let mut binding = Binding { broker, publisher: None, previous: None, view: None };
    while let Some(work) = shared.next() {
        match work {
            Work::Retire => binding.retire_idle(shared),
            Work::Publish(update) => {
                if let Err(error) = binding.publish(shared, &update) { shared.fail(error); break; }
                shared.complete(update.epoch);
            }
        }
    }
}
