use super::*;
use crate::provision::{tests::Fake, Entry, MutationError, Snapshot};
struct Fixture {
    backend: Fake,
    id: String,
    fail_prepare: bool,
    fail_finish: bool,
    outcomes: Vec<Outcome>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            backend: Fake::new(),
            id: uuid::Uuid::new_v4().to_string(),
            fail_prepare: false,
            fail_finish: false,
            outcomes: vec![],
        }
    }
}
impl Backend for Fixture {
    fn snapshot(&mut self) -> Result<Snapshot, String> {
        self.backend.snapshot()
    }
    fn journal(&mut self, record: &Journal) -> Result<(), String> {
        self.backend.journal(record)
    }
    fn create(&mut self, slot: u8, value: &Entry) -> Result<(), MutationError> {
        self.backend.create(slot, value)
    }
    fn remove(&mut self, slot: u8) -> Result<(), String> {
        self.backend.remove(slot)
    }
    fn refresh(&mut self) -> Result<(), String> {
        self.backend.refresh()
    }
    fn safe_to_restore(&mut self) -> Result<(), String> {
        self.backend.safe_to_restore()
    }
}
impl Operation for Fixture {
    fn prepare(&mut self) -> Result<(), String> {
        if self.fail_prepare {
            Err("preparation failed".into())
        } else {
            Ok(())
        }
    }
    fn id(&self) -> String {
        self.id.clone()
    }
    fn prior(&self) -> Result<Journal, String> {
        self.backend
            .journals
            .last()
            .cloned()
            .ok_or("No prior journal.".into())
    }
    fn finish(&mut self, outcome: &Outcome) -> Result<(), String> {
        self.outcomes.push(outcome.clone());
        if self.fail_finish {
            Err("storage failed".into())
        } else {
            Ok(())
        }
    }
}
#[test]
fn dispatch_owns_creation_and_verification_and_does_not_delete_committed_slots() {
    let mut f = Fixture::new();
    dispatch(&mut f, Action::Create(2), || Ok(())).unwrap();
    assert!(f.outcomes.last().unwrap().success);
    assert_eq!(f.backend.journals.last().unwrap().id, f.id);
    assert!(dispatch(&mut f, Action::Recover, || Ok(()))
        .unwrap_err()
        .contains("committed"));
    assert!(!f.outcomes.last().unwrap().success);
    f.backend.journals.last_mut().unwrap().committed = false;
    dispatch(&mut f, Action::Recover, || Ok(())).unwrap();
    assert!(f.snapshot().unwrap().entries.is_empty());
    let mut f = Fixture::new();
    f.fail_prepare = true;
    assert!(dispatch(&mut f, Action::Create(2), || Ok(())).is_err());
    assert!(f.backend.journals.is_empty());
    f.fail_prepare = false;
    f.fail_finish = true;
    assert!(dispatch(&mut f, Action::Create(2), || Ok(()))
        .unwrap_err()
        .contains("storage"));
    assert!(f.backend.journals.last().unwrap().committed); // Preserve successful persistent provisioning.
    assert!(dispatch(&mut f, Action::Recover, || Ok(()))
        .unwrap_err()
        .contains("Outcome storage"));
    assert!(dispatch(&mut Fixture::new(), Action::Recover, || Ok(())).is_err());
    assert!(dispatch(&mut Fixture::new(), Action::Create(2), || Err(
        "cancel".into()
    ))
    .is_err());
}
#[test]
fn private_entry_is_bounded_gated_and_never_falls_through_to_the_app() {
    let request = Request {
        operation: uuid::Uuid::new_v4().to_string(),
        owner: crate::windows_process::current().unwrap(),
        action: Action::Create(2),
    };
    let args: Vec<_> = request
        .arguments()
        .unwrap()
        .split(' ')
        .map(str::to_owned)
        .collect();
    assert_eq!(entry(&[], true, |_| panic!()), None);
    assert_eq!(entry(&[ARGUMENT.into()], true, |_| panic!()), Some(2));
    assert_eq!(
        entry(&args, false, |_| panic!("no elevation or mutation")),
        Some(78)
    );
    assert_eq!(entry(&args, true, |_| Ok(())), Some(0));
    assert_eq!(entry(&args, true, |_| Err("declined".into())), Some(1));
    assert!(helper_main().is_none());
    assert!(elevated().is_ok());
    let mut stale = request;
    stale.owner.created = "1".into();
    assert!(native(stale).is_err()); // Fails on token or parent identity, before any machine writes.
}

#[test]
fn elevated_dispatch_validates_requested_starter_layout_and_legacy_recovery() {
    let mut fixture = Fixture::new();
    dispatch(
        &mut fixture,
        Action::CreateLayout {
            slot: 2,
            layout: crate::descriptor::Layout::STARTER,
        },
        || Ok(()),
    )
    .unwrap();
    assert!(fixture.outcomes.last().unwrap().success);
    let journal = fixture.backend.journals.last().unwrap();
    assert_eq!(
        crate::descriptor::Layout::from_descriptor(
            2,
            journal.owned[&2].descriptor.as_deref().unwrap()
        )
        .unwrap(),
        crate::descriptor::Layout::STARTER
    );
    let mut fixture = Fixture::new();
    assert!(dispatch(
        &mut fixture,
        Action::CreateLayout {
            slot: 2,
            layout: crate::descriptor::Layout {
                axes: 0,
                buttons: 0,
                hats: 0
            }
        },
        || Ok(())
    )
    .is_err());
    assert!(fixture.backend.journals.is_empty());
}
