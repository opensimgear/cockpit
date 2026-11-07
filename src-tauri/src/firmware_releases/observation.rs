use super::{Resolver, State};
use crate::observations;
use osg_streams::{Publisher, TopicSpec};
use std::sync::PoisonError;

const TOPIC: &str = "companion.firmware-releases";

pub(super) struct ReleaseObservation {
    publisher: Publisher,
    runtime: observations::Runtime,
}

impl Resolver {
    pub(crate) fn observe_releases(&self, runtime: &observations::Runtime) -> Result<(), String> {
        // Register the baseline under the same lock used for commits.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.observation.is_some() {
            return Err("Companion firmware release observation is already installed.".into());
        }
        let publisher = runtime.broker().register(TopicSpec {
            topic: TOPIC.into(),
            source: "companion-release-resolver".into(),
            audiences: vec!["ui".into(), "browser".into(), "mcp".into()],
        }, self.project(&state), runtime.captured_us(), TOPIC.into()).map_err(|error| error.to_string())?;
        state.observation = Some(ReleaseObservation { publisher, runtime: runtime.clone() });
        Ok(())
    }

    pub(super) fn update_metadata<T>(&self, update: impl FnOnce(&mut State) -> T) -> T {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let before = state.observation.as_ref().map(|_| self.project(&state));
        let result = update(&mut state);
        if let Some(observer) = state.observation.as_ref() {
            let snapshot = self.project(&state);
            if before.as_ref() != Some(&snapshot) {
                let _ = observer.publisher.publish_latest_state(observer.publisher.generation(),
                    snapshot, observer.runtime.captured_us(), TOPIC);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firmware_releases::Release;
    use serde_json::json;

    #[test]
    fn retained_releases_publish_only_changed_metadata_on_all_transports() {
        let resolver = Resolver { development: false, ..Resolver::default() };
        let runtime = observations::Runtime::new();
        resolver.observe_releases(&runtime).unwrap();
        let second_runtime = observations::Runtime::new();
        assert!(resolver.observe_releases(&second_runtime).is_err());
        assert!(!second_runtime.broker().available_to(TOPIC, "ui"));
        assert!(Resolver::default().observe_releases(&runtime).is_err());
        let broker = runtime.broker();
        let mut readers = ["ui", "browser", "mcp"].map(|audience| {
            broker.subscribe(audience, audience, TOPIC).unwrap()
        });
        let release = Release { version: "2.3.4".into(), assets: Default::default() };
        for reader in &mut readers {
            assert_eq!(reader.try_recv().unwrap().unwrap().payload.unwrap()["firmwareChecked"], false);
        }
        resolver.commit_catalog(Ok(release.clone())).unwrap();
        for reader in &mut readers {
            assert_eq!(reader.try_recv().unwrap().unwrap().payload.unwrap()["firmwareVersion"], "2.3.4");
        }
        resolver.commit_catalog(Ok(release)).unwrap();
        for reader in &mut readers { assert!(reader.try_recv().unwrap().is_none()); }
        drop(readers);
        assert!(resolver.commit_catalog(Err("Catalog unavailable".into())).is_err());
        let mut reopened = broker.subscribe("reopened", "browser", TOPIC).unwrap();
        let baseline = reopened.try_recv().unwrap().unwrap().payload.unwrap();
        assert_eq!(baseline["firmwareVersion"], json!(null));
        assert_eq!(baseline["firmwareError"], "Catalog unavailable");
        assert_eq!(baseline, resolver.metadata());
    }
}
