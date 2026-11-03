use super::*;
use std::{sync::mpsc, time::Duration};

#[test]
fn game_cache_post_commit_artwork_preparation_cannot_block_profile_stop() {
    let manager = Arc::new(GatewayManager::mcp_test_manager());
    let client = "game-cache-stop-regression";
    let applied = manager.request(json!({"command":"profile_apply","clientId":client,
        "id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));
    assert_eq!(applied["ok"], true, "{applied}");
    assert!(manager.profile_session.lock().unwrap().status().owner.is_some());
    let tags = manager.games.status()["tags"].clone();

    let (entered, arrived) = mpsc::sync_channel(1);
    let (release, resumed) = mpsc::sync_channel(1);
    let resumed = Mutex::new(resumed);
    manager.games.set_artwork_prepare_hook(Some(Arc::new(move || {
        entered.send(()).unwrap();
        resumed.lock().unwrap().recv().unwrap();
    })));
    let editing = manager.clone();
    let metadata = std::thread::spawn(move || editing.request(json!({
        "command":"game_tag_definitions_save","clientId":client,"tags":{},"expectedTags":tags,
    })));
    let preparation_started = arrived.recv_timeout(Duration::from_secs(3));
    if preparation_started.is_err() {
        // Release any preparation which arrives after the deadline before
        // reporting failure, so a failed test cannot strand its native worker.
        manager.games.set_artwork_prepare_hook(None);
        let _ = release.send(());
        let response = metadata.join().unwrap();
        panic!("Metadata did not reach artwork preparation: {preparation_started:?}; {response}");
    }
    assert_eq!(manager.games.status()["tags"], json!({}), "Registry commit must precede preparation");

    let (stopped, stop_result) = mpsc::sync_channel(1);
    let stopping = manager.clone();
    let stop = std::thread::spawn(move || {
        let response = stopping.request(json!({"command":"profile_stop","clientId":client}));
        stopped.send(response).unwrap();
    });
    let prompt_stop = stop_result.recv_timeout(Duration::from_millis(250));
    // Always unblock and join before asserting the deadline, including when
    // a regression makes Stop wait for the simulated slow artwork file read.
    manager.games.set_artwork_prepare_hook(None);
    release.send(()).unwrap();
    let acknowledged = metadata.join().unwrap();
    let response = match prompt_stop {
        Ok(response) => response,
        Err(error) => {
            let response = stop_result.recv_timeout(Duration::from_secs(3));
            stop.join().unwrap();
            manager.shutdown();
            panic!("Profile Stop waited for artwork preparation: {error}; {response:?}");
        }
    };
    stop.join().unwrap();
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["state"]["profiles"]["profileSession"]["state"], "stopped");
    assert_eq!(acknowledged["ok"], true, "{acknowledged}");
    assert!(manager.profile_session.lock().unwrap().status().owner.is_none());
    manager.shutdown();
}
