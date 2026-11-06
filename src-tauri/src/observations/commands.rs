use super::*;
use tauri::Manager;

fn principal<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<String, String> {
    if !["main", "profile-activation"].contains(&window.label())
        || !crate::local_document(
            &window.url().map_err(|error| error.to_string())?,
            window.app_handle().config().build.dev_url.as_ref(),
        )
    {
        return Err("This window cannot observe Cockpit state.".into());
    }
    Ok(format!("window:{}", window.label()))
}

#[tauri::command]
pub(crate) fn observation_subscribe<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    state: tauri::State<'_, crate::NativeGateway>,
    topic: String,
    channel: tauri::ipc::Channel<Delivery>,
) -> Result<String, String> {
    let client = principal(&window)?;
    if window.label() == "profile-activation"
        && !matches!(topic.as_str(), "preferences.theme" | "preferences.progress" | crate::profile_activation_window::TOPIC)
    {
        return Err("The activation window cannot observe this topic.".into());
    }
    let controller = topic.starts_with("controllers.input.")
        .then(|| Mutex::new(super::controller_channel::ControllerChannel::default()));
    state.0.with_observation_topic(&topic, || state.0.observations
        .subscribe(&client, "ui", &topic, move |delivery| {
            let delivery = match &controller {
                Some(encoder) => encoder.lock().unwrap_or_else(|error| error.into_inner()).encode(delivery),
                None => delivery,
            };
            channel.send(delivery).map_err(|error| error.to_string())
        }))
}

#[tauri::command]
pub(crate) fn observation_acknowledge<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    state: tauri::State<'_, crate::NativeGateway>,
    subscription_id: String,
    delivery: String,
) -> Result<(), String> {
    state
        .0
        .observations
        .acknowledge(&principal(&window)?, &subscription_id, &delivery)
}

#[tauri::command]
pub(crate) fn observation_unsubscribe<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    state: tauri::State<'_, crate::NativeGateway>,
    subscription_id: String,
) -> Result<(), String> {
    state
        .0
        .observations
        .unsubscribe(&principal(&window)?, &subscription_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::{
        sync::{mpsc, Arc},
        time::Duration,
    };
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn owned_windows_receive_and_ack_passive_channels_with_exact_scope() {
        let manager = Arc::new(crate::gateways::GatewayManager::mcp_test_manager());
        manager
            .observation_runtime()
            .broker()
            .register(
                osg_streams::TopicSpec {
                    topic: "test.private".into(),
                    source: "test".into(),
                    audiences: vec!["ui".into()],
                },
                json!(0),
                1,
                "test-clock".into(),
            )
            .unwrap();
        let app = mock_builder()
            .manage(crate::NativeGateway(manager.clone()))
            .build(mock_context(noop_assets()))
            .unwrap();
        let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let progress =
            tauri::WebviewWindowBuilder::new(&app, "profile-activation", Default::default())
                .build()
                .unwrap();
        let unexpected = tauri::WebviewWindowBuilder::new(&app, "unexpected", Default::default())
            .build()
            .unwrap();
        let (send, receive) = mpsc::channel();
        let channel = tauri::ipc::Channel::new(move |body| {
            let value: Value = body.deserialize()?;
            send.send(value).unwrap();
            Ok(())
        });
        let token = observation_subscribe(
            main.clone(),
            main.state(),
            "preferences.theme".into(),
            channel,
        )
        .unwrap();
        let frame = receive.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(frame["subscriptionId"], token);
        assert_eq!(frame["frame"]["kind"], "snapshot");
        observation_acknowledge(main.clone(), main.state(), token.clone(), "1".into()).unwrap();
        assert!(observation_acknowledge(
            progress.clone(),
            progress.state(),
            token.clone(),
            "1".into()
        )
        .is_err());
        observation_unsubscribe(main.clone(), main.state(), token.clone()).unwrap();
        assert!(observation_acknowledge(main.clone(), main.state(), token, "1".into()).is_err());
        assert!(observation_subscribe(
            progress.clone(),
            progress.state(),
            "test.private".into(),
            tauri::ipc::Channel::new(|_| Ok(()))
        )
        .is_err());
        assert!(observation_subscribe(
            unexpected.clone(),
            unexpected.state(),
            "preferences.theme".into(),
            tauri::ipc::Channel::new(|_| Ok(()))
        )
        .is_err());
        let token = observation_subscribe(
            progress.clone(),
            progress.state(),
            "preferences.progress".into(),
            tauri::ipc::Channel::new(|_| Ok(())),
        )
        .unwrap();
        observation_unsubscribe(progress.clone(), progress.state(), token).unwrap();
        manager.shutdown();
    }

    #[test]
    fn a_remote_document_never_gains_native_observation() {
        let manager = Arc::new(crate::gateways::GatewayManager::mcp_test_manager());
        let app = mock_builder()
            .manage(crate::NativeGateway(manager.clone()))
            .build(mock_context(noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(
            &app,
            "main",
            tauri::WebviewUrl::External("https://example.com/".parse().unwrap()),
        )
        .build()
        .unwrap();
        assert!(observation_subscribe(
            window.clone(),
            window.state(),
            "preferences.theme".into(),
            tauri::ipc::Channel::new(|_| Ok(()))
        )
        .is_err());
        manager.shutdown();
    }
}
