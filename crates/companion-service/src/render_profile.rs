//! Bounded renderer metadata. It describes content support, never control or
//! transport authority. Engine semantics and budgets share the firmware manifest.
use crate::publication::{ensure, id};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::{collections::BTreeSet, sync::OnceLock};
mod compatibility;
pub use compatibility::RenderCompatibilityIssue;
pub(crate) use compatibility::check_document;

fn manifest() -> &'static Value {
    static VALUE: OnceLock<Value> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../packages/companion-designer/assets/widget-profiles.json")).expect("checked widget profile manifest"))
}
fn same_contract(expected:&Value,value:&Value)->bool{
    if expected.is_number(){return value.as_f64().is_some_and(|number|Some(number)==expected.as_f64());}
    if let Some(fields)=expected.as_object(){return value.as_object().is_some_and(|actual|actual.len()==fields.len()&&fields.iter().all(|(key,expected)|same_contract(expected,&value[key])));}
    value==expected
}
pub fn validate(value: &Value) -> Result<Value, String> {
    let object=value.as_object().ok_or("Missing renderer profile.")?;
    ensure(object.len()==11&&object.keys().all(|key|matches!(key.as_str(),
        "formatVersion"|"engine"|"engineVersion"|"fontSet"|"textFormat"|"features"|"id"|"limits"|"firmwareVersion"|"boardProfile"|"screen")),"Invalid renderer profile fields.")?;
    let source=manifest();
    for key in ["formatVersion","engine","engineVersion","fontSet","textFormat"] {
        ensure(same_contract(&source[key],&value[key]),&format!("Unsupported renderer {key}."))?;
    }
    let profile=source["profiles"].as_object().unwrap().values().find(|profile|profile["id"]==value["id"]).ok_or("Unknown renderer profile ID.")?;
    ensure(same_contract(&profile["limits"],&value["limits"]),"Renderer limits do not match its versioned profile.")?;
    let features=value["features"].as_array().ok_or("Invalid renderer features.")?;
    let expected=source["features"].as_array().unwrap();
    ensure(features.len()==expected.len()&&features.iter().all(|feature|expected.contains(feature))
        &&features.iter().map(Value::to_string).collect::<BTreeSet<_>>().len()==features.len(),"Unsupported or repeated renderer feature.")?;
    ensure(value["firmwareVersion"].as_str().is_some_and(|version|!version.is_empty()&&version.len()<=64&&version.bytes().all(|byte|(33..=126).contains(&byte)))
        &&value["boardProfile"].as_str().is_some_and(id),"Invalid renderer firmware/board identity.")?;
    let screen=value["screen"].as_object().ok_or("Missing renderer geometry.")?;
    ensure(screen.len()==2&&["width","height"].iter().all(|key|value["screen"][key].as_f64().is_some_and(|size|size.fract()==0.0&&(64.0..=1024.0).contains(&size))),"Invalid renderer geometry.")?;
    let mut result=value.clone();result["features"]=source["features"].clone();result["limits"]=profile["limits"].clone();
    for key in ["formatVersion","engine","engineVersion","fontSet","textFormat"]{result[key]=source[key].clone();}
    for key in ["width","height"]{result["screen"][key]=Value::from(value["screen"][key].as_f64().unwrap() as u64);}
    Ok(result)
}

#[cfg(test)]
pub(crate) fn test_profile(rich:bool)->Value {
    let source=manifest();let profile=&source["profiles"][if rich{"rich"}else{"compact"}];
    json!({"formatVersion":source["formatVersion"],"engine":source["engine"],"engineVersion":source["engineVersion"],
        "fontSet":source["fontSet"],"textFormat":source["textFormat"],"features":source["features"],"id":profile["id"],"limits":profile["limits"],
        "firmwareVersion":"2026.10.22.1","boardProfile":"headless","screen":{"width":800,"height":480}})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_profiles_are_bounded_and_preserve_the_advertised_semantics() {
        for rich in [false,true] {
            let good=test_profile(rich);let original=good.clone();assert_eq!(validate(&good).unwrap(),good);assert_eq!(original,good);
            let mut reordered=good.clone();reordered["features"].as_array_mut().unwrap().reverse();assert_eq!(validate(&reordered).unwrap(),good);
            let mut floating=good.clone();floating["screen"]["width"]=json!(800.0);floating["engineVersion"]=json!(1.0);floating["limits"]["pages"]=json!(4.0);assert_eq!(validate(&floating).unwrap(),good);
            for (key,value) in [("engineVersion",json!(2)),("id",json!("unknown")),("fontSet",json!("system-fonts")),("firmwareVersion",json!("x".repeat(65))),
                ("boardProfile",json!("bad board")),("token",json!("credential")),("screen",json!({"width":800,"height":480,"extra":true}))] {
                let mut bad=good.clone();bad[key]=value;assert!(validate(&bad).is_err(),"{key}");
            }
            for value in [json!(0),json!(1025),json!(64.5),json!("800")] {
                let mut bad=good.clone();bad["screen"]["width"]=value;assert!(validate(&bad).is_err());
            }
            let mut bad=good.clone();bad["limits"]["fontSize"]=json!(4096);assert!(validate(&bad).is_err());
            let mut bad=good.clone();bad["features"][0]=bad["features"][1].clone();assert!(validate(&bad).is_err());
            assert!(validate(&Value::Null).is_err());
        }
    }
    #[test]
    fn shared_profile_admission_oracle_matches_firmware_and_designer() {
        let fixture:Value=serde_json::from_str(include_str!("../../../apps/companion-esp32/test/render-profiles.json")).unwrap();
        for profile in fixture["valid"].as_array().unwrap(){
            assert!(validate(profile).is_ok());
            for mutation in fixture["mutations"].as_array().unwrap(){
                let mut value=profile.clone();let path=mutation["path"].as_array().unwrap();let mut target=&mut value;
                for key in &path[..path.len()-1]{target=&mut target[key.as_str().unwrap()];}
                target[path.last().unwrap().as_str().unwrap()]=mutation["value"].clone();
                assert_eq!(validate(&value).is_ok(),mutation["valid"]==true,"{mutation}");
            }
        }
    }
}
