//! Small cached activity projection, separate from UI artwork and vehicle pages.
use super::*;
impl GamesRuntime {
    pub(crate) fn activity_snapshot(&self) -> Value {
        let mut catalogue=self.inner.settings.lock().unwrap_or_else(|p|p.into_inner()).catalogue.clone();
        for game in &mut catalogue {
            let id=game["id"].as_str().unwrap_or("").to_owned();
            let setup=crate::game_discovery::integration_binding(game).map(|key|self.inner.setup.activity_status(key)).unwrap_or(Value::Null);
            game["integrationSetup"]=setup;
            game["integrationContext"]=self.inner.contexts.lock().unwrap_or_else(|p|p.into_inner()).view(&id);
            game["profileContext"]=self.inner.activation.view(&id);
            game["currentVehicle"]=self.inner.vehicles.lock().unwrap_or_else(|p|p.into_inner()).current(&id).unwrap_or(Value::Null);
        }
        let review=self.inner.discovery.lock().unwrap_or_else(|p|p.into_inner());
        let diagnostics=review.diagnostics.iter().take(8).cloned().collect::<Vec<_>>();
        let libraries=review.libraries.iter().map(|library|json!({"id":library.id,"status":library.status,"found":library.found})).collect::<Vec<_>>();
        let review=json!({"scanId":review.scan_id,"status":review.status,"proposed":review.entries.len(),"diagnostics":diagnostics,"libraries":libraries,
            "artworkReady":review.entries.iter().filter(|entry|entry.image.is_some()).count(),"tagged":review.entries.iter().filter(|entry|!entry.tag_ids.is_empty()).count()});
        json!({"catalogue":catalogue,"discovery":review})
    }
}
#[cfg(test)]
mod activity_tests {
    use super::*;
    #[test]
    fn cached_projection_keeps_raw_artwork_and_reads_native_review_vehicle_context() {
        let root=crate::test_directory();let runtime=GamesRuntime::test_runtime(Arc::new(|_|json!({"ok":true})),Paths::in_directory(root.path()),Arc::new(crate::game_discovery::NativePlatform));
        runtime.inner.settings.lock().unwrap().catalogue=vec![json!({"id":"racing","name":"Racing","provider":"assetto","integrationKey":"assetto-corsa","integrationCompatible":true,"defaultBackground":{"image":"artwork:stable.jpg"}})];
        runtime.set_test_vehicle("racing",Some("GT3"));
        {let mut review=runtime.inner.discovery.lock().unwrap();review.scan_id="scan-1".into();review.status="ready".into();review.diagnostics.push("One library could not be read.".into());}
        let state=runtime.activity_snapshot();assert_eq!(state["catalogue"][0]["defaultBackground"]["image"],"artwork:stable.jpg");assert_eq!(state["catalogue"][0]["currentVehicle"]["id"],"GT3");assert_eq!(state["discovery"]["status"],"ready");assert_eq!(state["discovery"]["diagnostics"][0],"One library could not be read.");assert!(!state.to_string().contains("data:image"));assert!(state["catalogue"][0]["profileContext"].is_object());
        runtime.shutdown();
    }
}
