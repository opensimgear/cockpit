use super::*;
use crate::profile_session::{RunOwner, State, Status};

#[test]
fn lifecycle_enqueue_and_action_abandon_advance_writer_wake() {
    let wake=Arc::new(osg_streams::NativeWake::default());
    let (sender,receiver)=mpsc::channel();
    let registry=Arc::new(Mutex::new(reconcile::Reconciler::default()));
    let mut observer=session::Observer::new(sender,registry.clone(),wake.clone());
    let mut status=Status{state:State::Stopped,owner:None,applied_snapshot:None,degradation:vec![],fault:None};
    observer.observe(&status);
    status.owner=Some(RunOwner{session:"wake-test".into(),generation:1,profile_id:"profile".into(),profile_revision:1,game_id:None});
    status.state=State::Running;
    let before_lifecycle=wake.observed();
    observer.observe(&status);
    assert!(wake.observed()>before_lifecycle);
    assert!(receiver.try_recv().is_ok());

    let action=Action{key:"abandoned".into(),registry,wake:wake.clone(),identity:None};
    let before_abandon=wake.observed();
    drop(action);
    assert!(wake.observed()>before_abandon);
}
