use super::*;
use uuid::Uuid;

const CANCELLED: &str = "Backup creation was cancelled.";

#[derive(PartialEq, Eq)]
enum Phase {
    Preparing,
    Cancelled,
    Committing,
}

pub(super) struct CreateOperation {
    id: Uuid,
    phase: Phase,
}

impl CreateOperation {
    fn new() -> Self {
        Self { id: Uuid::new_v4(), phase: Phase::Preparing }
    }

    fn reserve_commit(&mut self) -> Result<(), String> {
        if self.phase == Phase::Cancelled {
            return Err(CANCELLED.into());
        }
        self.phase = Phase::Committing;
        Ok(())
    }
}

impl Runtime {
    pub(super) fn create_id(&self) -> Option<Uuid> {
        self.creating.lock().unwrap_or_else(|error| error.into_inner())
            .as_ref().map(|operation| operation.id)
    }

    pub(super) fn check_create_cancelled(&self) -> Result<(), String> {
        let active = self.creating.lock().unwrap_or_else(|error| error.into_inner());
        if active.as_ref().is_some_and(|operation| operation.phase == Phase::Cancelled) {
            return Err(CANCELLED.into());
        }
        Ok(())
    }

    pub(crate) fn cancel_create(&self, id: Uuid) -> Result<Value, String> {
        let mut active = self.creating.lock().unwrap_or_else(|error| error.into_inner());
        let operation = active.as_mut().filter(|operation| operation.id == id)
            .ok_or("That backup creation is no longer active.")?;
        if operation.phase == Phase::Committing {
            return Err("Backup creation is finalizing.".into());
        }
        operation.phase = Phase::Cancelled;
        Ok(json!({"createId":id,"cancelRequested":true}))
    }

    // The caller owns controls for the whole creation. Cancellation only uses
    // the short operation lock, including at the durable commit boundary.
    pub(super) fn run_create(&self, expected: u64) -> Result<Value, String> {
        *self.creating.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(CreateOperation::new());
        self.publish_observation("creating");
        let result = self.create_locked(expected, || {
            self.creating.lock().unwrap_or_else(|error| error.into_inner())
                .as_mut().ok_or("Backup operation is unavailable.")?.reserve_commit()
        });
        let operation = self.creating.lock().unwrap_or_else(|error| error.into_inner()).take();
        if operation.is_some_and(|operation| operation.phase == Phase::Cancelled) {
            self.publish_observation("cancelled");
            return Err(CANCELLED.into());
        }
        let result = result.and_then(|entry| Ok(json!({"backup":entry,"status":self.status()?})));
        self.publish_observation(if result.is_ok() { "complete" } else { "failed" });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_cancel_fences_commit_and_cleans_prepared_bundle() {
        let directory = crate::test_directory();
        let database = Database::open(directory.path().join("cockpit.sqlite3")).unwrap();
        let backup = Runtime::fixture(database, directory.path().to_owned());
        let operation = CreateOperation::new();
        let id = operation.id;
        *backup.creating.lock().unwrap() = Some(operation);
        assert!(backup.cancel_create(Uuid::new_v4()).is_err());

        let result = backup.create_locked(0, || {
            assert_eq!(backup.cancel_create(id)?["cancelRequested"], true);
            assert!(backup.cancel_create(id).is_ok());
            backup.creating.lock().unwrap().as_mut().unwrap().reserve_commit()
        });
        assert_eq!(result.err().unwrap(), CANCELLED);
        assert_eq!(backup.status().unwrap()["revision"], 0);
        assert_eq!(fs::read_dir(directory.path().join("backups")).unwrap().count(), 0);
    }

    #[test]
    fn reserved_commit_rejects_late_cancellation() {
        let directory = crate::test_directory();
        let database = Database::open(directory.path().join("cockpit.sqlite3")).unwrap();
        let backup = Runtime::fixture(database, directory.path().to_owned());
        let mut operation = CreateOperation::new();
        operation.reserve_commit().unwrap();
        let id = operation.id;
        *backup.creating.lock().unwrap() = Some(operation);
        assert_eq!(backup.cancel_create(id).unwrap_err(), "Backup creation is finalizing.");
    }
}
