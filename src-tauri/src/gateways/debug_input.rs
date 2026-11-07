use super::*;
use crate::profile_debug_controller::Request;

impl GatewayManager {
    pub(super) fn dispatch_debug_controller(&self, request: &Request) -> Result<Value, String> {
        let state = self
            .profile_session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .debug_controller(request)?;
        Ok(self.finish_debug_controller(state))
    }

    fn finish_debug_controller(&self, mut state: Value) -> Value {
        let (queued, changed) = debug_controller_flags(&mut state);
        self.notify_debug_controller(queued, changed);
        state
    }

    fn notify_debug_controller(&self, queued: bool, changed: bool) {
        if queued {
            self.profile_wake.notify();
        }
        if changed {
            if let Some(publisher) = &self.debug_inputs_observation {
                let _ = publisher.publish_latest_state(
                    publisher.generation(),
                    json!({}),
                    self.observations.captured_us(),
                    "profile-debug-inputs",
                );
            }
        }
    }

    #[cfg(all(windows, debug_assertions, feature = "reactive-qualification"))]
    pub(crate) fn qualify_debug_controller_batch<T>(
        &self,
        offer: impl FnOnce(
            &mut dyn FnMut(&Request) -> Result<(Value, std::time::Duration), String>,
            std::time::Duration,
        ) -> Result<T, String>,
    ) -> Result<T, String> {
        use std::time::Instant;
        // Use the same admission order as the ordinary non-mutating gateway route.
        let _profile = self
            .profile_admission
            .read()
            .unwrap_or_else(|p| p.into_inner());
        let _game = self
            .game_admission
            .read()
            .unwrap_or_else(|p| p.into_inner());
        if self.closed.load(Ordering::Acquire) {
            return Err("Cockpit gateway manager has closed.".into());
        }
        let waiting = Instant::now();
        let mut session = self
            .profile_session
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let wait = waiting.elapsed();
        let (mut queued, mut changed) = (false, false);
        let result = offer(
            &mut |request| {
                let entered = Instant::now();
                let mut state = session.debug_controller(request)?;
                let work = entered.elapsed();
                let (frame_queued, frame_changed) = debug_controller_flags(&mut state);
                queued |= frame_queued;
                changed |= frame_changed;
                Ok((state, work))
            },
            wait,
        );
        drop(session);
        self.notify_debug_controller(queued, changed);
        result
    }
}

fn debug_controller_flags(state: &mut Value) -> (bool, bool) {
    let Some(fields) = state.as_object_mut() else {
        return (false, false);
    };
    (
        fields.remove("queued").is_some_and(|queued| queued == true),
        fields
            .remove("changed")
            .is_some_and(|changed| changed == true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisoned_coordinator_still_rejects_an_obsolete_debug_owner() {
        let manager = GatewayManager::mcp_test_manager();
        std::thread::scope(|scope| {
            let poisoned = scope.spawn(|| {
                let _session = manager.profile_session.lock().unwrap();
                panic!("inject coordinator lock poison");
            });
            assert!(poisoned.join().is_err());
        });

        let request = Request::parse(&json!({
            "command":"profile_debug_controller_status", "nodeId":"debug",
            "owner":{"session":uuid::Uuid::new_v4(), "profileId":uuid::Uuid::new_v4(),
                "generation":1, "profileRevision":1, "gameId":null}
        }))
        .unwrap();
        assert!(manager
            .dispatch_debug_controller(&request)
            .unwrap_err()
            .contains("owner expired"));
        #[cfg(all(windows, debug_assertions, feature = "reactive-qualification"))]
        assert!(manager
            .qualify_debug_controller_batch(|admit, _| admit(&request))
            .unwrap_err()
            .contains("owner expired"));
        manager.shutdown();
    }
}
