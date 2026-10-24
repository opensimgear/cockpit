use super::*;

pub(super) fn usb_panel_snapshot(companion: &str, devices: &[Value]) -> Option<ProfilePanelOperation> {
    let mut matching = devices.iter().filter(|usb|usb["connected"] == true && usb["ready"] == true
        && usb["device"]["companionId"] == companion);
    let usb = matching.next()?;
    if matching.next().is_some() { return None; }
    let reported=&usb["device"]["operation"];
    let operation = osg_companion_service::operation::validate_panel_operation(
        reported,reported["instanceId"].as_str()?,reported["publicationHash"].as_str()?).ok()?;
    if operation["attemptId"].as_u64().is_none_or(|attempt|attempt == 0)
        || operation.get("sessionId").is_some_and(|session|session != &usb["device"]["sessionId"])
        || operation.get("generation").is_some_and(|generation|generation != &usb["device"]["generation"]) { return None; }
    Some(ProfilePanelOperation { operation, live_session:false,
        fresh:usb["statusAgeMs"].as_u64().is_some_and(|age|age < 1500) })
}
pub(super) fn usb_panel_operation(assignment: &Value, devices: &[Value]) -> Option<ProfilePanelOperation> {
    let report=usb_panel_snapshot(assignment["companionId"].as_str()?,devices)?;
    osg_companion_service::operation::validate_panel_operation(&report.operation,
        assignment["instanceId"].as_str()?,assignment["publicationHash"].as_str()?).ok()?;
    Some(report)
}
pub(super) fn usb_render_profile(usb:&Value)->Option<Value>{
    if usb["connected"]!=true||usb["ready"]!=true{return None;}
    let device=&usb["device"];let profile=osg_companion_service::render_profile::validate(&device["renderProfile"]).ok()?;
    (profile["firmwareVersion"]==device["firmwareVersion"]&&profile["boardProfile"]==device["boardProfile"]).then_some(profile)
}
pub(super) fn describe_usb_presence(host:&mut Value,devices:&[Value]){
    let assignments=host["instances"].as_array().cloned().unwrap_or_default();
    if let Some(clients)=host["clients"].as_array_mut(){for client in clients{
        let Some(companion)=client["companionId"].as_str().filter(|id|!id.is_empty()) else{continue};
        let matching=|usb:&&Value|usb["device"]["companionId"]==companion;
        // A recovered device can return on a different port while the old link
        // still holds diagnostic history. Prefer its current live observation.
        let Some(usb)=devices.iter().filter(matching).find(|usb|usb["connected"]==true && usb["ready"]==true)
            .or_else(||devices.iter().filter(matching).find(|usb|usb_recovery_message(usb).is_some())) else{continue};
        client["sessionConnected"]=client["connected"].clone();
        if let Some(message)=usb_recovery_message(usb){
            client["connectionStatus"]=json!(message);
            if usb["connected"]!=true || usb["ready"]!=true {continue;}
        }
        client["connected"]=json!(true);
        if let Some(profile)=usb_render_profile(usb){
            if client["renderProfileError"].is_null(){
                if !client["renderProfile"].is_null()&&client["renderProfile"]!=profile{
                    client["renderProfile"]=Value::Null;client["renderProfileError"]=json!("USB and authenticated connections report conflicting renderer profiles.");
                }else{client["renderProfile"]=profile;}
            }
        }else if !usb["device"]["renderProfile"].is_null() || (!client["renderProfile"].is_null()&&client["renderProfile"]["firmwareVersion"]!=usb["device"]["firmwareVersion"]){
            client["renderProfile"]=Value::Null;client["renderProfileError"]=json!("The USB renderer report does not match its firmware identity.");
        }
        if let Some(transports)=client["transports"].as_array_mut(){if !transports.contains(&json!("usb")){transports.push(json!("usb"));}}
        else{client["transports"]=json!(["usb"]);}
        client["connectionStatus"]=json!(usb_recovery_message(usb).unwrap_or_else(||usb["device"]["statusMessage"].as_str().unwrap_or_else(||match usb["device"]["status"].as_str(){
            Some("authenticating")=>"Connecting to Cockpit",
            Some("opening-panel")=>"Loading panel",
            Some("error")=>"Cockpit connection or session failed",
            Some("offline")=>"USB connected; panel session stopped",
            Some("awaiting-assignment")=>"No panel assigned",
            _=>"",
        })));
        if client["operation"].is_null() && usb["device"]["operation"].is_object()
            && assignments.iter().any(|assignment|assignment["companionId"]==client["companionId"]
                && assignment["instanceId"]==usb["device"]["operation"]["instanceId"]
                && assignment["publicationHash"]==usb["device"]["operation"]["publicationHash"]) {
            client["operation"]=usb["device"]["operation"].clone();
        }
        if usb["device"]["status"]=="panel-error"{
            client["panelError"]=json!(usb["device"]["statusMessage"].as_str().unwrap_or("The screen could not load or render this panel."));
        }
    }}
}
pub(super) fn usb_recovery_message(usb:&Value)->Option<&str>{
    if !matches!(usb["recovery"]["phase"].as_str(),Some("waiting"|"connecting"|"waiting-for-device")){return None;}
    usb["recovery"]["message"].as_str().filter(|message|!message.is_empty()&&message.len()<=191&&!message.chars().any(char::is_control))
}
pub(super) fn screen_dimensions(profile:&str)->Option<(u64,u64)>{
    match profile {"sunton-3248s035r"=>Some((480,320)),"waveshare-lcd7-s3"=>Some((800,480)),_=>None}
}
pub(super) fn validate_screen_document(document:&Value,size:(u64,u64))->Result<(),String>{
    let widgets=document["presentation"]["widgets"].as_array().ok_or("Panel has no widgets.")?;
    if document["presentation"]["viewports"].as_array().is_some_and(|v|v.iter().any(|v|v.get("backgroundAssetId").is_some())) || widgets.iter().any(|w|w["config"].get("iconAssetId").is_some()){
        return Err("Use image widgets instead of backgrounds or button icons.".into());
    }
    if document["presentation"]["groups"].as_array().is_some_and(|g|!g.is_empty()) || widgets.iter().any(|w|w.get("groupId").is_some()){
        return Err("ESP32 panels do not support groups. Ungroup widgets before assigning.".into());
    }
    if widgets.len()>32 || serde_json::to_vec(document).map_err(|e|e.to_string())?.len()>16384{return Err("ESP32 panel exceeds the 32 widget or 16 KB document limit.".into());}
    let image_bytes:f64=widgets.iter().filter(|w|w["kind"]=="image").map(|w|{
        let screen=document["presentation"]["viewports"].as_array().and_then(|v|v.iter().find(|v|v["pageId"]==w["pageId"]));
        screen.map(|s|{let m=&s["safeInsets"];(w["bounds"]["width"].as_f64().unwrap_or(0.0)*size.0 as f64*(1.0-m["left"].as_f64().unwrap_or(0.0)-m["right"].as_f64().unwrap_or(0.0))).floor()*(w["bounds"]["height"].as_f64().unwrap_or(0.0)*size.1 as f64*(1.0-m["top"].as_f64().unwrap_or(0.0)-m["bottom"].as_f64().unwrap_or(0.0))).floor()*3.0}).unwrap_or(0.0)
    }).sum();
    if image_bytes>1200.0*1024.0{return Err("Image widgets exceed the 1200 KB device pixel budget.".into());}
    for w in widgets{
        if w["kind"]=="image" && w["config"]["assetId"].as_str().is_none(){return Err("Choose an asset for every image widget.".into());}
        let text_ok=|v:&Value|v.as_str().is_some_and(|s|s.bytes().all(|c|c.is_ascii_alphanumeric()||b" .,:;-_+=|/%!()?".contains(&c)));
        if !text_ok(&w["label"]) || ["prefix","suffix","unit"].iter().any(|key|w["config"].get(*key).is_some_and(|v|!text_ok(v))){return Err("ESP32 labels and text must use the supported device font.".into());}
        if !matches!(w["kind"].as_str(),Some("button"|"indicator"|"text"|"numeric"|"gauge"|"image")){return Err("This widget type is not supported on ESP32 screens.".into());}
        if w["kind"]=="button" && (w["config"]["buttonMode"]!="momentary" || w["config"]["pulseMs"]!=0 || w["config"]["repeatMs"]!=0){return Err("ESP32 buttons must be momentary without pulse or repeat.".into());}
        if w["kind"]=="button"{if let Some(screen)=document["presentation"]["viewports"].as_array().and_then(|screens|screens.iter().find(|v|v["pageId"]==w["pageId"])){
            let width=screen["width"].as_f64().unwrap_or(1.0);let height=screen["height"].as_f64().unwrap_or(1.0);
            let scale=(size.0 as f64/width).min((size.1 as f64)/height);
            let insets=&screen["safeInsets"];
            if (w["bounds"]["width"].as_f64().unwrap_or(0.0)*(1.0-insets["left"].as_f64().unwrap_or(0.0)-insets["right"].as_f64().unwrap_or(0.0))*width*scale).floor()<44.0 || (w["bounds"]["height"].as_f64().unwrap_or(0.0)*(1.0-insets["top"].as_f64().unwrap_or(0.0)-insets["bottom"].as_f64().unwrap_or(0.0))*height*scale).floor()<44.0{return Err("ESP32 buttons must render at least 44 by 44 pixels. Enlarge the button.".into());}
        }}
        if matches!(w["kind"].as_str(),Some("numeric"|"gauge")) && (w["config"]["precision"].as_u64().is_none_or(|p|p>4)||w["config"]["min"].as_f64().is_none_or(|n|n< -1e9)||w["config"]["max"].as_f64().is_none_or(|n|n>1e9)){return Err("ESP32 numeric precision must be 0-4 and ranges must stay within +/-1,000,000,000.".into());}
    }
    let pages=document["ui"]["pages"].as_array().ok_or("Panel has no pages.")?;
    if pages.len()>4{return Err("ESP32 panels support at most 4 pages.".into());}
    let screens=document["presentation"]["viewports"].as_array().ok_or("Panel has no screen size.")?;
    if pages.is_empty()||pages.iter().any(|page|!screens.iter().any(|screen|screen["pageId"]==page["id"]&&screen["width"].as_u64()==Some(size.0)&&screen["height"].as_u64()==Some(size.1))){
        return Err(format!("Every panel page must be {} × {} px to match this ESP32 screen.",size.0,size.1));
    }
    Ok(())
}
pub(super) fn target_render_profile(client:&Value,firmware:&Value)->Result<Option<Value>,String>{
    if let Some(error)=client["renderProfileError"].as_str().filter(|error|!error.is_empty()){return Err(error.into());}
    if !client["renderProfile"].is_null(){return osg_companion_service::render_profile::validate(&client["renderProfile"]).map(Some);}
    if client["connected"]==true{return Ok(None);}
    if let Ok(profile)=osg_companion_service::render_profile::validate(&firmware["renderProfile"]){
        if profile["firmwareVersion"]==firmware["version"]&&profile["boardProfile"]==firmware["profile"]{return Ok(Some(profile));}
    }
    Ok(None)
}
pub(super) fn validate_target_panel(hub:&Hub,hash:&str,client:&Value,firmware:&Value)->Result<(),String>{
    if hub.publication_document(hash)?["schemaVersion"]==4 {
        return if client["kind"]=="esp32" {Err("Compiled widgets are currently available for browser companions. ESP32 support is in the backlog.".into())} else {Ok(())};
    }
    if let Some(profile)=target_render_profile(client,firmware)?{
        if let Some(issue)=hub.publication_renderer_issues(hash,&profile)?.first(){return Err(issue.message.clone());}
        return Ok(());
    }
    if client["kind"]!="esp32"{return Ok(());}
    let size=client["screen"]["width"].as_u64().zip(client["screen"]["height"].as_u64())
        .or_else(||screen_dimensions(firmware["profile"].as_str().unwrap_or("")))
        .ok_or("Connect this ESP32 screen by USB to identify its screen size before assigning a panel.")?;
    validate_screen_document(&hub.publication_document(hash)?,size)
}
