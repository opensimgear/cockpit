//! Admission for native designer snapshots; portable publication formats stay unchanged.
use osg_companion_service::{validate_widget_bundle, Publication, PublicationUpload};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
pub(super) const LIMIT: usize = 64 * 1024 * 1024;
fn ensure(ok: bool, message: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(message.into()) } }
fn exact(v: &Value, fields: &[&str]) -> Result<(), String> {
    ensure(v.as_object().is_some_and(|o| o.len()==fields.len() && fields.iter().all(|k|o.contains_key(*k))), "Unexpected or missing designer field.")
}
fn id(v: &Value) -> Result<&str, String> {
    let s=v.as_str().ok_or("Invalid designer ID.")?;
    ensure(!s.is_empty() && s.len()<=64 && s.as_bytes()[0].is_ascii_alphanumeric() && s.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_' || b==b'-'), "Invalid designer ID.")?; Ok(s)
}
fn title(v: &Value) -> Result<(), String> {
    ensure(v.as_str().is_some_and(|s|!s.trim().is_empty() && s.encode_utf16().count()<=120 && !s.chars().any(|c|c<='\u{1f}')), "Invalid designer title.")
}
fn list(v: &Value, min: usize, max: usize) -> Result<&Vec<Value>, String> {
    let rows=v.as_array().ok_or("Invalid designer list.")?;
    ensure((min..=max).contains(&rows.len()), "Designer list exceeds its storage budget.")?; Ok(rows)
}
fn canonical(v: &Value) -> Result<String, String> { serde_json_canonicalizer::to_string(v).map_err(|_|"Cannot canonicalize designer content.".into()) }
fn hash(v: &Value) -> Result<String, String> { Ok(format!("{:x}",Sha256::digest(canonical(v)?.as_bytes()))) }
fn parameters(source: &Value, overrides: &Value) -> Result<Value, String> {
    let overrides=overrides.as_object().ok_or("Invalid widget parameters.")?;
    let definitions=source["parameters"].as_array().ok_or("Missing widget parameters.")?;
    ensure(overrides.keys().all(|key|definitions.iter().any(|p|p["id"]==*key)), "Unknown widget parameter.")?;
    let mut result=serde_json::Map::new();
    for p in definitions {
        let key=p["id"].as_str().ok_or("Invalid parameter identity.")?;
        let value=overrides.get(key).filter(|v|!v.is_null()).unwrap_or(&p["default"]);
        let valid=match p["type"].as_str() {
            Some("number")=>value.as_f64().is_some_and(|v|v.is_finite() && v>=p["min"].as_f64().unwrap() && v<=p["max"].as_f64().unwrap()),
            Some("boolean")=>value.is_boolean(),
            Some(kind @ ("text" | "color"))=>value.as_str().is_some_and(|s|s.encode_utf16().count()<=120 && !s.chars().any(|c|c<='\u{1f}') && (kind=="text" || ((s.len()==7 || s.len()==9) && s.starts_with('#') && s[1..].bytes().all(|b|b.is_ascii_hexdigit())))),
            _=>false,
        };
        ensure(valid,"Widget parameter is outside its declared domain.")?;
        result.insert(key.into(),value.clone());
    }
    Ok(Value::Object(result))
}
fn draft(v: &Value, sources: &[Value]) -> Result<(), String> {
    exact(v,&["id","title","pages","widgets"])?; id(&v["id"])?; title(&v["title"])?;
    let pages=list(&v["pages"],1,16)?; let widgets=list(&v["widgets"],0,192)?;
    let mut ids=BTreeSet::new();
    for page in pages {
        let mut fields=vec!["id","title","width","height"];if page.get("background").is_some(){fields.push("background");let color=page["background"].as_str().unwrap_or("");ensure(color.len()==7&&color.starts_with('#')&&color[1..].bytes().all(|b|b.is_ascii_hexdigit()),"Invalid page background.")?;}exact(page,&fields)?;
        ensure(ids.insert(id(&page["id"])?),"Duplicate designer ID.")?; title(&page["title"])?;
        for key in ["width","height"] { ensure(page[key].as_u64().is_some_and(|n|(320..=8192).contains(&n)),"Invalid page dimensions.")?; }
    }
    for widget in widgets {
        exact(widget,&["id","pageId","label","sourceId","parameters","bounds"])?;
        ensure(ids.insert(id(&widget["id"])?),"Duplicate designer ID.")?; title(&widget["label"])?;
        ensure(pages.iter().any(|p|p["id"]==widget["pageId"]),"Unknown widget page.")?;
        let source=sources.iter().find(|s|s["id"]==widget["sourceId"]).ok_or("Unavailable widget source.")?;
        parameters(source,&widget["parameters"])?;
        let b=&widget["bounds"]; exact(b,&["x","y","width","height"])?;
        for key in ["x","y","width","height"] { ensure(b[key].as_f64().is_some_and(|n|n.is_finite() && (0.0..=1.0).contains(&n)),"Invalid widget bounds.")?; }
        ensure(b["width"].as_f64().unwrap()>0. && b["height"].as_f64().unwrap()>0. && b["x"].as_f64().unwrap()+b["width"].as_f64().unwrap()<=1.+1e-10 && b["y"].as_f64().unwrap()+b["height"].as_f64().unwrap()<=1.+1e-10,"Widget lies outside its page.")?;
    }
    Ok(())
}
fn matches_draft(draft: &Value, sources: &[Value], publication: &Value) -> Result<(), String> {
    let doc=&publication["document"];
    let pages=draft["pages"].as_array().unwrap(); let widgets=draft["widgets"].as_array().unwrap();
    let expected_pages=pages.iter().map(|p|json!({"id":p["id"],"title":p["title"]})).collect::<Vec<_>>();
    let viewports=pages.iter().map(|p|{let mut viewport=json!({"pageId":p["id"],"width":p["width"],"height":p["height"]});if let Some(background)=p.get("background"){viewport["background"]=background.clone();}viewport}).collect::<Vec<_>>();
    ensure(doc["ui"]["id"]==draft["id"] && doc["ui"]["title"]==draft["title"] && doc["ui"]["pages"]==json!(expected_pages) && doc["presentation"]["viewports"]==json!(viewports) && doc["presentation"]["widgets"].as_array().unwrap().len()==widgets.len(),"Saved publication belongs to an earlier panel. Recompile before saving.")?;
    for widget in widgets {
        let placed=doc["presentation"]["widgets"].as_array().unwrap().iter().find(|w|w["id"]==widget["id"]).ok_or("Missing compiled widget.")?;
        ensure(["pageId","label","bounds"].iter().all(|k|placed[*k]==widget[*k]),"Saved publication belongs to an earlier placement.")?;
        let source=sources.iter().find(|s|s["id"]==widget["sourceId"]).unwrap();
        let artifact=&doc["presentation"]["artifacts"].as_array().unwrap().iter().find(|a|a["hash"]==placed["artifactHash"]).ok_or("Missing compiled artifact.")?["artifact"];
        ensure(artifact["sourceHash"]==hash(source)? && artifact["parameterHash"]==hash(&parameters(source,&widget["parameters"])?)?,"Saved publication belongs to earlier widget source or parameters.")?;
    }
    Ok(())
}
pub(super) fn validate(value: &Value) -> Result<(), String> {
    ensure(serde_json::to_vec(value).map_err(|_|"Cannot encode designer workspace.")?.len()<=LIMIT,"Designer storage exceeds 64 MiB.")?;
    exact(value,&["sources","drafts","publications"])?;
    let sources=list(&value["sources"],0,256)?;
    let bundle=json!({"definitions":sources});
    validate_widget_bundle(&bundle)?;
    ensure(canonical(&value["sources"])?.len()<=8*1024*1024,"Widget library exceeds 8 MiB.")?;
    let drafts=list(&value["drafts"],0,20)?; let mut ids=BTreeSet::new();
    for d in drafts { draft(d,sources)?; ensure(ids.insert(id(&d["id"])?),"Duplicate saved panel.")?; }
    let publications=value["publications"].as_object().ok_or("Invalid publication archive.")?;
    let mut count=0;
    for (key, revisions) in publications {
        id(&json!(key))?;
        for p in list(revisions,0,100)? {
            count+=1; ensure(count<=100,"Publication archive exceeds 100 revisions.")?;
            exact(p,&["document","assets","publicationHash","capabilityFingerprint"])?;
            let checked=Publication::validate(PublicationUpload {
                canonical_document:canonical(&p["document"])?, publication_hash:p["publicationHash"].as_str().ok_or("Missing publication hash.")?.into(),
                capability_fingerprint:Some(p["capabilityFingerprint"].as_str().ok_or("Missing capability fingerprint.")?.into()),
                assets:serde_json::from_value(p["assets"].clone()).map_err(|_|"Invalid publication assets.")?,
            })?;
            ensure(checked.ui_id==*key,"Publication belongs to another panel.")?;
        }
    }
    for d in drafts {
        if let Some(latest)=publications.get(d["id"].as_str().unwrap()).and_then(|r|r.as_array()).and_then(|r|r.last()) { matches_draft(d,sources,latest)?; }
    }
    Ok(())
}
