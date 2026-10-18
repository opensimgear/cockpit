//! Bounded visibility phase admission and persistent cleanup progress.
use super::*;
use crate::profile_session_visibility::{Progress, Visibility};
use std::time::Duration;

pub(super) fn active(visibility: &mut Visibility) -> Result<(), String> {
    if visibility.is_empty() || visibility.poll()? == Progress::Active {
        Ok(())
    } else {
        Err("The owned device visibility session is no longer active.".into())
    }
}
fn wait(
    visibility: &mut Visibility,
    restoring: bool,
    timeout: Duration,
    mut guard: impl FnMut() -> Result<(), String>,
    mut renew: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    if visibility.is_empty() {
        return Ok(());
    }
    let deadline = Instant::now() + timeout;
    let mut progress = if restoring {
        visibility.stop()?
    } else {
        guard()?;
        visibility.start()?
    };
    loop {
        if progress
            == if restoring {
                Progress::Restored
            } else {
                Progress::Active
            }
        {
            // The last poll may have completed native verification. Renew
            // before handing control back to the next durable Apply phase.
            if !restoring {
                guard()?;
                renew()?;
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(if restoring {"Owned visibility restoration is still pending; explicit recovery must finish before another profile can run."}
                else {"Owned visibility did not become active before its deadline."}.into());
        }
        guard()?;
        renew()?;
        std::thread::sleep(Duration::from_millis(5));
        progress = if restoring {
            visibility.stop()?
        } else {
            visibility.poll()?
        };
    }
}
impl NativeProfileHost {
    pub(super) fn start_visibility(&mut self, owner: &RunOwner) -> Result<(), String> {
        let epoch = self.admission_epoch.clone();
        let state = self.run(owner)?;
        let expected = state.admission_epoch;
        wait(
            &mut state.visibility,
            false,
            Duration::from_secs(3),
            || Self::check_epoch(&epoch, expected),
            || {
                state
                    .output
                    .as_mut()
                    .map_or(Ok(()), |output| output.keep_alive())
            },
        )
    }
    pub(super) fn restore_visibility(&mut self, run: &OwnedRun) -> Result<(), String> {
        if !run.attempted.contains(&ApplyPhase::Visibility) {
            return Ok(());
        }
        let owner_key = key(&run.owner);
        if let Some(state) = self.prepared.get_mut(&owner_key) {
            return wait(
                &mut state.visibility,
                true,
                Duration::from_secs(3),
                || Ok(()),
                || Ok(()),
            );
        }
        if !self.recovery_visibility.contains_key(&owner_key) {
            if let Some(visibility) = Visibility::from_receipts(run, self.controllers.clone())? {
                self.recovery_visibility
                    .insert(owner_key.clone(), visibility);
            } else {
                return Ok(());
            }
        }
        // Retain the object on pending/error: the next recovery polls the same
        // native worker instead of cancelling and restarting restoration.
        wait(
            self.recovery_visibility.get_mut(&owner_key).unwrap(),
            true,
            Duration::from_secs(3),
            || Ok(()),
            || Ok(()),
        )?;
        self.recovery_visibility.remove(&owner_key);
        Ok(())
    }
}
#[cfg(test)]
#[path = "profile_session_visibility_host_tests.rs"]
mod tests;
