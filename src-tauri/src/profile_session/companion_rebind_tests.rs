use super::*;
use std::time::{Duration, Instant};

fn owner() -> RunOwner {
    RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        profile_id: "profile".into(),
        profile_revision: 1,
        game_id: None,
    }
}
fn finished(
    preparation: &mut Preparation<usize>,
    owner: &RunOwner,
    hash: &Option<String>,
) -> Result<usize, String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(result) = preparation.poll(owner, hash) {
            return result;
        }
        assert!(Instant::now() < deadline, "Preparation failed to finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn blocked_capture_and_compilation_never_block_health_or_spawn_duplicate_workers() {
    let mut preparation = Preparation::default();
    let owner = owner();
    let hash = Some("program".into());
    let (entered, started) = mpsc::sync_channel(1);
    let (release, blocked) = mpsc::sync_channel(1);
    assert!(preparation.start(1, owner.clone(), hash.clone(), move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(2)).unwrap();
        Ok(42)
    }));
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    // Both operations return while the same capture/compiler is still blocked.
    assert!(preparation.poll(&owner, &hash).is_none());
    assert!(!preparation.ready(1_000_000));
    assert!(
        !preparation.start(1_000_000, owner.clone(), hash.clone(), || panic!(
            "duplicate worker"
        ))
    );
    release.send(()).unwrap();
    assert_eq!(finished(&mut preparation, &owner, &hash).unwrap(), 42);
    assert!(!preparation.ready(100_000));
    assert!(preparation.ready(100_001));
}

#[test]
fn stale_owner_or_program_drops_preparation_without_admitting_a_result() {
    for changed_hash in [false, true] {
        let mut preparation = Preparation::default();
        let owner = owner();
        let hash = Some("program".into());
        let (release, blocked) = mpsc::sync_channel(1);
        let (done, completed) = mpsc::sync_channel(1);
        assert!(preparation.start(1, owner.clone(), hash.clone(), move || {
            blocked.recv_timeout(Duration::from_secs(2)).unwrap();
            done.send(()).unwrap();
            Ok(42)
        }));
        let mut current = owner.clone();
        let mut current_hash = hash.clone();
        if changed_hash {
            current_hash = Some("replacement".into());
        } else {
            current.generation += 1;
        }
        assert!(preparation
            .poll(&current, &current_hash)
            .unwrap()
            .unwrap_err()
            .contains("owner expired"));
        release.send(()).unwrap();
        completed.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(preparation.poll(&owner, &hash).is_none());
        assert!(preparation.start(100_001, current.clone(), current_hash.clone(), || Ok(73)));
        assert_eq!(
            finished(&mut preparation, &current, &current_hash).unwrap(),
            73
        );
    }
}

#[test]
fn capture_errors_and_panics_complete_without_poisoning_future_retries() {
    let mut preparation = Preparation::<usize>::default();
    let owner = owner();
    let hash = Some("program".into());
    preparation.start(1, owner.clone(), hash.clone(), || {
        Err("capture unavailable".into())
    });
    assert_eq!(
        finished(&mut preparation, &owner, &hash).unwrap_err(),
        "capture unavailable"
    );
    preparation.start(100_001, owner.clone(), hash.clone(), || {
        panic!("compiler ended")
    });
    assert!(finished(&mut preparation, &owner, &hash)
        .unwrap_err()
        .contains("worker ended"));
    preparation.start(200_001, owner.clone(), hash.clone(), || Ok(1));
    assert_eq!(finished(&mut preparation, &owner, &hash).unwrap(), 1);
}
