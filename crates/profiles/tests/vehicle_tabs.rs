use osg_profiles::{ProfileDocument, VehicleTab, validate_document};
use serde_json::json;

fn document()->ProfileDocument {
    let mut document=osg_profiles::default_document();
    document.eligibility.game_ids=vec!["dcs".into()];
    for id in ["base","hornet","viper"] {
        document.resources.push(serde_json::from_value(json!({"id":id,"name":id,"kind":"device","required":false,"ports":[]})).unwrap());
        document.editor.positions.push(serde_json::from_value(json!({"nodeId":id,"x":0,"y":0})).unwrap());
    }
    document.vehicle_tabs=vec![tab("hornet","FA-18C_hornet"),tab("viper","F-16C_50")];
    document
}
fn tab(id:&str,vehicle:&str)->VehicleTab {VehicleTab {id:id.into(),game_id:"dcs".into(),vehicle_id:vehicle.into(),vehicle_name:id.into(),node_ids:vec![id.into()]}}

#[test]
fn old_profiles_round_trip_and_loaded_vehicle_derives_only_base_and_matching_nodes() {
    assert!(serde_json::to_value(osg_profiles::default_document()).unwrap().get("vehicleTabs").is_none());
    let document=document();validate_document(&document).unwrap();
    assert_eq!(ProfileDocument::parse(&document.export().unwrap()).unwrap(),document);
    for (game,vehicle,expected) in [(None,None,vec!["base"]),(Some("dcs"),Some("FA-18C_hornet"),vec!["base","hornet"]),(Some("dcs"),Some("F-16C_50"),vec!["base","viper"]),(Some("dcs"),Some("unknown"),vec!["base"]),(Some("assetto-corsa-rally"),Some("F-16C_50"),vec!["base"])] {
        let active=document.for_vehicle(game,vehicle);
        assert!(active.vehicle_tabs.is_empty());
        assert_eq!(active.resources.iter().map(|resource|resource.id.as_str()).collect::<Vec<_>>(),expected);
        assert_eq!(active.editor.positions.len(),expected.len());
    }
}
#[test]
fn tabs_reject_unknown_shared_nodes_duplicate_vehicles_and_missing_game_context() {
    let valid=document();
    let mut invalid=valid.clone();invalid.vehicle_tabs[0].node_ids.push("missing".into());assert!(validate_document(&invalid).is_err());
    let mut invalid=valid.clone();invalid.vehicle_tabs[0].node_ids.push("viper".into());assert!(validate_document(&invalid).is_err());
    let mut invalid=valid.clone();invalid.vehicle_tabs[0].vehicle_id=invalid.vehicle_tabs[1].vehicle_id.clone();assert!(validate_document(&invalid).is_err());
    let mut invalid=valid.clone();invalid.eligibility.game_ids.clear();assert!(validate_document(&invalid).is_err());
    let mut invalid=valid;invalid.vehicle_tabs[0].vehicle_id="bad\nvehicle".into();assert!(validate_document(&invalid).is_err());
}
#[test]
fn semantic_identity_tracks_vehicle_matching_and_membership_but_ignores_tab_labels_and_order() {
    let original=document();let hash=original.semantic_hash().unwrap();
    let mut renamed=original.clone();renamed.vehicle_tabs[0].vehicle_name="Hornet".into();renamed.vehicle_tabs.reverse();assert_eq!(renamed.semantic_hash().unwrap(),hash);
    renamed.vehicle_tabs[0].vehicle_id="different".into();assert_ne!(renamed.semantic_hash().unwrap(),hash);
}

#[test]
fn executable_vehicle_projection_removes_paths_belonging_to_inactive_connections() {
    let mut graph=document();
    for resource in &mut graph.resources {
        resource.ports=serde_json::from_value(json!([{"id":"value","name":"Value","direction":if resource.id=="base"{"output"}else{"input"},"contract":{"delivery":"state","schema":{"type":"boolean"},"freshnessMs":1000,"fallback":false}}])).unwrap();
    }
    graph.bindings=serde_json::from_value(json!([
        {"id":"hornet-path","from":{"nodeId":"base","portId":"value"},"to":{"nodeId":"hornet","portId":"value"}},
        {"id":"viper-path","from":{"nodeId":"base","portId":"value"},"to":{"nodeId":"viper","portId":"value"}},
    ])).unwrap();
    graph.editor.wire_routes=serde_json::from_value(json!([
        {"bindingId":"hornet-path","points":[{"x":0,"y":0},{"x":100,"y":0}]},
        {"bindingId":"viper-path","points":[{"x":0,"y":0},{"x":100,"y":0}]},
    ])).unwrap();
    validate_document(&graph).unwrap();
    let hornet=graph.for_vehicle(Some("dcs"),Some("FA-18C_hornet"));
    assert_eq!(hornet.editor.wire_routes.len(),1);assert_eq!(hornet.editor.wire_routes[0].binding_id,"hornet-path");
    validate_document(&hornet).unwrap();
    assert!(graph.for_vehicle(None,None).editor.wire_routes.is_empty());
}
