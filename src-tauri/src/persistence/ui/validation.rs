use serde::{Deserialize,Serialize};
use serde_json::Value;
#[derive(Clone,Debug,Deserialize,Serialize,PartialEq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct WindowGeometry {pub x:i32,pub y:i32,pub width:u32,pub height:u32,pub scale_factor:f64,pub maximized:bool}
fn exact(value:&Value,required:&[&str],optional:&[&str])->Result<(),String>{
    let object=value.as_object().ok_or("UI preferences must be an object.")?;
    if required.iter().any(|key|!object.contains_key(*key))||object.keys().any(|key|!required.contains(&key.as_str())&&!optional.contains(&key.as_str())){return Err("Unknown or missing UI preference fields.".into());}Ok(())
}
fn text(value:&Value,max:usize)->bool{value.as_str().is_some_and(|s|!s.is_empty()&&s.len()<=max&&!s.chars().any(char::is_control))}
fn ensure(valid:bool)->Result<(),String>{if valid{Ok(())}else{Err("Invalid UI preferences. Preserve their SQLite record.".into())}}
pub(crate) fn stale_observations(key:&str,value:&mut Value){
    if key!="remembered"{return;}let Some(rows)=value.get_mut("components").and_then(Value::as_array_mut)else{return};
    for component in rows {
        component["presence"]=Value::String("unknown".into());component["stale"]=Value::Bool(true);component["capture"]=Value::String("unknown".into());
        for key in ["owner","inputOwner","configurationOwner","configurationRoute"]{component[key]=Value::Null;}
        component["forget"]["allowed"]=Value::Bool(false);component["forget"]["reason"]=Value::String("Refresh this provider before forgetting a remembered component.".into());
        for connection in component["connections"].as_array_mut().unwrap(){connection["current"]=Value::Bool(false);}
    }
}
pub(crate) fn validate(key:&str,value:&Value)->Result<(),String>{match key {
    "theme"=>ensure(value.is_boolean()),
    "devices-mode"=>ensure(matches!(value.as_str(),Some("list"|"topology"))),
    "canvas-navigation"=>{exact(value,&["wheelAction"],&[])?;ensure(matches!(value["wheelAction"].as_str(),Some("pan"|"zoom")))},
    "canvas-styling"=>{exact(value,&["arrowStyle","cornerRadius","collapseParallelWires"],&[])?;ensure(matches!(value["arrowStyle"].as_str(),Some("smooth"|"orthogonal"))&&value["cornerRadius"].as_f64().is_some_and(|n|n.is_finite()&&(0.0..=100.0).contains(&n))&&value["collapseParallelWires"].is_boolean())},
    "tray"=>{exact(value,&["closeToTray","minimizeToTray"],&[])?;ensure(value["closeToTray"].is_boolean()&&value["minimizeToTray"].is_boolean())},
    "progress"=>{exact(value,&["delivery","position"],&[])?;ensure(matches!(value["delivery"].as_str(),Some("system"|"window"))&&matches!(value["position"].as_str(),Some("bottom-right"|"bottom-left"|"top-right"|"top-left")))},
    "windows"=>{if value.is_null(){return Ok(());}let geometry:WindowGeometry=serde_json::from_value(value.clone()).map_err(|_|"Invalid window geometry.")?;ensure(geometry.x.unsigned_abs()<=100_000&&geometry.y.unsigned_abs()<=100_000&&(160..=32768).contains(&geometry.width)&&(120..=32768).contains(&geometry.height)&&geometry.scale_factor.is_finite()&&(0.25..=8.0).contains(&geometry.scale_factor))},
    "remembered"=>inventory(value),_=>Err("Unknown UI preference owner.".into()),
}}
fn inventory(value:&Value)->Result<(),String>{
    if value.is_null(){return Ok(());}exact(value,&["components","forgotten"],&[])?;
    let components=value["components"].as_array().filter(|rows|rows.len()<=1024).ok_or("Invalid remembered component budget.")?;
    let mut ids=std::collections::BTreeSet::new();
    for component in components {
        exact(component,&["id","name","type","role","presence","hidden","enabled","owner","capture","status","diagnostics","connections","targets","evidence","references","firstSeen","lastSeen","observedAt","provider","stale","forget"],&["inputOwner","configurationOwner","configurationRoute"])?;
        ensure(text(&component["id"],512)&&ids.insert(component["id"].as_str().unwrap())&&text(&component["name"],512)&&component["role"].as_str().is_some_and(|s|s.len()<=256)&&text(&component["provider"],64)&&component["status"].as_str().is_some_and(|s|s.len()<=4096))?;
        ensure(matches!(component["type"].as_str(),Some("host"|"controller"|"osg"|"gateway"|"virtual"|"integration"|"companion"))&&matches!(component["presence"].as_str(),Some("connected"|"offline"|"unknown"|"unavailable"))&&matches!(component["capture"].as_str(),Some("ready"|"waiting"|"unavailable"|"unknown"))&&component["stale"].is_boolean())?;
        for key in ["firstSeen","lastSeen","observedAt"]{ensure(component[key].as_u64().is_some_and(|n|n<=9_007_199_254_740_991))?;}
        for key in ["hidden","enabled"]{ensure(component[key].is_null()||component[key].is_boolean())?;}
        for key in ["owner","inputOwner","configurationOwner","configurationRoute"]{ensure(component[key].is_null()||text(&component[key],512))?;}
        for key in ["diagnostics","connections","targets","evidence","references"]{ensure(component[key].as_array().is_some_and(|rows|rows.len()<=256))?;}
        for key in ["diagnostics","evidence"]{for row in component[key].as_array().unwrap(){ensure(row.as_str().is_some_and(|s|s.len()<=4096))?;}}
        for row in component["connections"].as_array().unwrap(){
            exact(row,&["id","from","to","transport","current"],&["observed","endpoint"])?;
            ensure(["id","from","to"].iter().all(|key|text(&row[*key],1024))&&matches!(row["transport"].as_str(),Some("usb"|"can"|"network"|"bluetooth"|"software"|"unknown"))&&row["current"].is_boolean()&&row.get("observed").is_none_or(Value::is_boolean)&&row.get("endpoint").is_none_or(|v|v.as_str().is_some_and(|s|s.len()<=2048)))?;
        }
        for row in component["targets"].as_array().unwrap(){exact(row,&["id","kind"],&["gatewayId"])?;ensure(text(&row["id"],1024)&&matches!(row["kind"].as_str(),Some("module"|"gateway"|"controller"|"usb"|"integration"|"companion"|"virtual"|"visibility"))&&row.get("gatewayId").is_none_or(|v|text(v,512)))?;}
        for row in component["references"].as_array().unwrap(){exact(row,&["kind","id","name"],&[])?;ensure(text(&row["kind"],256)&&text(&row["id"],512)&&row["name"].as_str().is_some_and(|s|s.len()<=512))?;}
        exact(&component["forget"],&["allowed","reason","effect"],&[])?;ensure(component["forget"]["allowed"].is_boolean()&&component["forget"]["reason"].as_str().is_some_and(|s|s.len()<=4096)&&component["forget"]["effect"].as_str().is_some_and(|s|s.len()<=4096))?;
    }
    let forgotten=value["forgotten"].as_object().filter(|rows|rows.len()<=4096).ok_or("Invalid inventory tombstone budget.")?;
    for(id,tombstone)in forgotten {ensure(!id.is_empty()&&id.len()<=512&&!id.chars().any(char::is_control))?;exact(tombstone,&["signature","sawAbsent"],&["type","provider"])?;ensure(tombstone["signature"].as_str().is_some_and(|s|s.len()<=8192)&&tombstone["sawAbsent"].is_boolean()&&tombstone.get("provider").is_none_or(|v|text(v,64))&&tombstone.get("type").is_none_or(|v|matches!(v.as_str(),Some("host"|"controller"|"osg"|"gateway"|"virtual"|"integration"|"companion"))))?;}
    Ok(())
}
