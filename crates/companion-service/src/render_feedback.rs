//! Compact render-v2 feedback transport. This module preserves native route authority:
//! callers supply a native validated envelope and its trusted source publication.
//! It creates no input source and never updates a feedback sample directly.
use crate::feedback::{feedback_descriptors, validate_feedback_samples, FeedbackBinding, FeedbackKind, FeedbackProvenance, FeedbackSample, FeedbackSource, FeedbackStatus, FeedbackValue};
use crate::publication::{digest_id, ensure, id};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const STEADY_ACK_BYTES: usize = 512;
pub const STARTUP_ACK_BYTES: usize = 3072;
pub const MAX_RENDER_ACK_BYTES: usize = 4096;
fn source_outputs(document: &Value) -> Result<&Vec<Value>,String> {
    document["ui"]["feedbackWidgets"].as_array().filter(|v|v.len()<=64).ok_or_else(||"Invalid compact feedback source outputs.".into())
}
fn output_index(document:&Value,channel:&str,output:&str)->Result<usize,String>{
    source_outputs(document)?.iter().position(|v|v["id"]==output&&v["channelId"]==channel).ok_or_else(||"Feedback output is absent from the trusted source.".into())
}
/// Short fields: v=2, g=binding generation, s=feedback sequence, m=0 snapshot/1 delta,
/// b=snapshot semantic-output indices, x=[index,status,provenance,source,age,value?].
/// Status fresh/unknown/stale/disconnected=0/1/2/3; provenance and source use the
/// stable enums requested/local/accepted/submitted/game=0..4, host/sim/script=0..2.
pub fn compact_feedback(envelope:&Value,source_document:&Value)->Result<Value,String>{
    crate::hub::exact(envelope,&["version","type","clientId","sessionId","generation","publicationHash","controllerInstanceId","sequence","bindingGeneration","mode","values"],&["bindings"])?;
    ensure(envelope["version"]==1&&envelope["type"]=="feedback","Invalid native feedback envelope.")?;
    for key in ["clientId","sessionId","controllerInstanceId"]{ensure(envelope[key].as_str().is_some_and(id),"Invalid compact feedback identity.")?;}
    ensure(envelope["publicationHash"].as_str().is_some_and(digest_id),"Invalid compact feedback publication.")?;
    for key in ["generation","sequence","bindingGeneration"]{crate::hub::counter(&envelope[key])?;}
    let snapshot=envelope["mode"]=="snapshot";
    ensure(snapshot||envelope["mode"]=="delta","Invalid compact feedback mode.")?;
    let schema=feedback_descriptors(source_document)?;
    let samples:Vec<FeedbackSample>=serde_json::from_value(envelope["values"].clone()).map_err(|_|"Invalid native feedback values.")?;
    let bindings:Vec<FeedbackBinding>=if snapshot{serde_json::from_value(envelope["bindings"].clone()).map_err(|_|"Snapshot bindings are required.")?}
        else{ensure(envelope.get("bindings").is_none(),"Delta cannot replace feedback bindings.")?;samples.iter().map(|v|FeedbackBinding{channel_id:v.channel_id.clone(),output_id:v.output_id.clone()}).collect()};
    let mut channels=BTreeSet::new();let mut bound_indices=Vec::new();
    for binding in &bindings{
        ensure(channels.insert(binding.channel_id.as_str()),"Duplicate compact feedback binding channel.")?;
        bound_indices.push(output_index(source_document,&binding.channel_id,&binding.output_id)?);
    }
    validate_feedback_samples(&schema,&bindings,&samples)?;
    ensure(!snapshot||samples.len()==bindings.len(),"Compact snapshot must contain every bound output.")?;
    let mut compact_values=Vec::new();
    for sample in samples{
        let index=output_index(source_document,&sample.channel_id,&sample.output_id)?;
        let descriptor=schema.iter().find(|v|v.channel_id==sample.channel_id).ok_or("Missing source feedback descriptor.")?;
        let canonical_unit=descriptor.unit.as_deref().unwrap_or(if matches!(descriptor.kind,FeedbackKind::Text){"text"}else{"state"});
        ensure(sample.unit==canonical_unit,"Compact feedback requires the source's canonical unit.")?;
        let status=match sample.status{FeedbackStatus::Fresh=>0,FeedbackStatus::Unknown=>1,FeedbackStatus::Stale=>2,FeedbackStatus::Disconnected=>3};
        let provenance=match sample.provenance{FeedbackProvenance::Requested=>0,FeedbackProvenance::Local=>1,FeedbackProvenance::HostAccepted=>2,FeedbackProvenance::HostSubmitted=>3,FeedbackProvenance::GameConfirmed=>4};
        let source=match sample.source_kind{FeedbackSource::Host=>0,FeedbackSource::Simulator=>1,FeedbackSource::Script=>2};
        let mut item=vec![json!(index),json!(status),json!(provenance),json!(source),json!(sample.source_age_ms)];
        if let Some(value)=sample.value{
            item.push(match value{
                FeedbackValue::Boolean(value)=>json!(value),
                FeedbackValue::Text(value)=>{ensure(value.len()<=120&&!value.chars().any(|c|c as u32<=31||c as u32==127),"Compact text exceeds its UTF-8 byte/control budget.")?;json!(value)},
                FeedbackValue::Number(value)=>{
                    let quantized=(value*1_000_000.0).round()/1_000_000.0;
                    ensure(quantized.is_finite()&&descriptor.range.as_ref().is_some_and(|r|quantized>=r.min&&quantized<=r.max),"Quantized feedback leaves the source range.")?;
                    json!(if quantized==0.0{0.0}else{quantized})
                }
            });
        }
        compact_values.push(Value::Array(item));
    }
    let mut compact=json!({"v":2,"g":envelope["bindingGeneration"],"s":envelope["sequence"],"m":if snapshot{0}else{1},"x":compact_values});
    if snapshot{compact["b"]=json!(bound_indices);}
    Ok(compact)
}
/// Stages the complete ACK and commits only when it fits. Oversized snapshots
/// fail atomically; the caller must reject/reconfigure, never split or truncate.
pub fn attach_render_feedback(reply:&mut Value,envelope:&Value,source_document:&Value,max_ack_bytes:usize)->Result<(),String>{
    ensure(max_ack_bytes>0&&max_ack_bytes<=MAX_RENDER_ACK_BYTES,"Invalid compact ACK byte budget.")?;
    ensure(reply.is_object()&&reply["ok"]==true&&reply["type"]=="ack"&&reply["ackStage"]=="source-accepted"&&reply["submitted"]==false,"Invalid compact source ACK.")?;
    for key in ["clientId","sessionId","generation","publicationHash"]{ensure(reply[key]==envelope[key]&&!reply[key].is_null(),"Compact feedback ACK identity differs from its native envelope.")?;}
    let compact=compact_feedback(envelope,source_document)?;
    let mut candidate=reply.clone();let object=candidate.as_object_mut().ok_or("Invalid compact ACK object.")?;
    object.remove("feedback");object.remove("feedbackSequence");object.insert("renderFeedback".into(),compact);
    ensure(serde_json::to_vec(&candidate).map_err(|e|e.to_string())?.len()<=max_ack_bytes,"Compact feedback ACK exceeds the negotiated byte budget.")?;
    *reply=candidate;Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    fn source()->Value{serde_json::from_str::<Value>(include_str!("../../../packages/companion-renderer/test/poc/sunton/A/source-publication.json")).unwrap()["document"].clone()}
    fn frame(snapshot:bool)->Value{
        let mut v=json!({"version":1,"type":"feedback","clientId":"esp32-123456","sessionId":"session-1","generation":"1","publicationHash":format!("sha256:{}","1".repeat(64)),"controllerInstanceId":"instance-1","sequence":"5","bindingGeneration":"2","mode":if snapshot{"snapshot"}else{"delta"},
        "values":[{"channelId":"reactor-power","outputId":"power-readout","value":37.12345678,"status":"fresh","provenance":"game-confirmed","sourceKind":"simulator","sourceAgeMs":8,"unit":"%","range":{"min":0,"max":100}},
        {"channelId":"laser-confirmed","outputId":"laser-state","value":true,"status":"fresh","provenance":"host-accepted","sourceKind":"host","sourceAgeMs":10,"unit":"state"}]});
        if snapshot{v["bindings"]=json!([{"channelId":"reactor-power","outputId":"power-readout"},{"channelId":"laser-confirmed","outputId":"laser-state"}]);}v
    }
    fn ack(frame:&Value)->Value{json!({"ok":true,"type":"ack","clientId":frame["clientId"],"sessionId":frame["sessionId"],"generation":frame["generation"],"publicationHash":frame["publicationHash"],"sequence":"7","ackStage":"source-accepted","submitted":false})}
    #[test]fn compact_full_snapshot_and_bounded_ack(){
        let frame=frame(true);let compact=compact_feedback(&frame,&source()).unwrap();
        assert_eq!(compact["b"],json!([0,2]));assert_eq!(compact["x"][0],json!([0,0,4,1,8,37.123457]));assert_eq!(compact["x"][1],json!([2,0,2,0,10,true]));
        let mut reply=ack(&frame);reply["feedback"]=frame.clone();attach_render_feedback(&mut reply,&frame,&source(),STARTUP_ACK_BYTES).unwrap();
        assert!(reply.get("feedback").is_none());assert!(serde_json::to_vec(&reply).unwrap().len()<STEADY_ACK_BYTES);
        let mut delta=ack(&frame);attach_render_feedback(&mut delta,&self::frame(false),&source(),STEADY_ACK_BYTES).unwrap();
    }
    #[test]fn complete_snapshot_and_transport_budget_are_atomic(){
        let mut frame=frame(true);frame["values"].as_array_mut().unwrap().pop();assert!(compact_feedback(&frame,&source()).is_err());
        let frame=self::frame(true);let mut reply=ack(&frame);let old=reply.clone();
        assert!(attach_render_feedback(&mut reply,&frame,&source(),10).is_err());assert_eq!(reply,old);
        assert!(attach_render_feedback(&mut reply,&frame,&source(),MAX_RENDER_ACK_BYTES+1).is_err());
        reply["sessionId"]=json!("wrong");assert!(attach_render_feedback(&mut reply,&frame,&source(),STARTUP_ACK_BYTES).is_err());
    }
    #[test]fn malformed_identity_routing_metadata_type_and_counters_are_rejected(){
        let mutations:Vec<Box<dyn Fn(&mut Value)>>=vec![
            Box::new(|v|v["version"]=json!(2)),Box::new(|v|v["extra"]=json!(1)),Box::new(|v|v["generation"]=json!("01")),Box::new(|v|v["sequence"]=json!("18446744073709551616")),
            Box::new(|v|v["bindingGeneration"]=json!(1)),Box::new(|v|v["clientId"]=json!(" bad")),Box::new(|v|v["publicationHash"]=json!("bad")),Box::new(|v|v["mode"]=json!("partial")),
            Box::new(|v|v["values"][0]["outputId"]=json!("unknown")),Box::new(|v|v["values"][0]["status"]=json!("invalid")),Box::new(|v|v["values"][0]["provenance"]=json!("invalid")),Box::new(|v|v["values"][0]["sourceKind"]=json!("invalid")),
            Box::new(|v|v["values"][0]["sourceKind"]=json!("host")),Box::new(|v|v["values"][0]["sourceAgeMs"]=json!(86_400_001)),Box::new(|v|v["values"][0]["unit"]=json!("kW")),Box::new(|v|v["values"][0]["range"]["max"]=json!(101)),
            Box::new(|v|v["values"][0]["value"]=json!(true)),Box::new(|v|v["values"][0]["value"]=json!(101)),Box::new(|v|v["values"][0]["status"]=json!("unknown")),Box::new(|v|{v["values"][0].as_object_mut().unwrap().remove("value");}),
            Box::new(|v|v["bindings"][1]=v["bindings"][0].clone()),Box::new(|v|v["values"][1]=v["values"][0].clone()),
        ];
        for mutate in mutations{let mut value=frame(true);mutate(&mut value);assert!(compact_feedback(&value,&source()).is_err(),"{value}");}
        let mut delta=frame(false);delta["bindings"]=json!([]);assert!(compact_feedback(&delta,&source()).is_err());
    }
    #[test]fn all_status_provenance_source_cases_preserve_unavailability(){
        for(status,number)in[("unknown",1),("stale",2),("disconnected",3)]{
            let mut frame=frame(false);frame["values"]=json!([{"channelId":"laser-confirmed","outputId":"laser-state","status":status,"provenance":"local","sourceKind":"script","sourceAgeMs":42,"unit":"state"}]);
            let compact=compact_feedback(&frame,&source()).unwrap();assert_eq!(compact["x"][0],json!([2,number,1,2,42]));
        }
        let mut frame=frame(false);frame["values"][0]["provenance"]=json!("requested");assert_eq!(compact_feedback(&frame,&source()).unwrap()["x"][0][2],0);
        frame["values"][0]["provenance"]=json!("host-submitted");assert_eq!(compact_feedback(&frame,&source()).unwrap()["x"][0][2],3);
    }
    #[test]fn text_utf8_budget_and_ack_shape_are_explicit(){
        let document=json!({"ui":{"feedbackWidgets":[{"id":"message","channelId":"channel","kind":"text"}]},"presentation":{"widgets":[{"feedbackIds":["message"]}]}});
        let mut value=frame(false);value["values"]=json!([{"channelId":"channel","outputId":"message","status":"fresh","value":"ready","provenance":"local","sourceKind":"script","sourceAgeMs":0,"unit":"text"}]);
        for text in ["a".repeat(120),"é".repeat(60)]{value["values"][0]["value"]=json!(text);assert!(compact_feedback(&value,&document).is_ok());}
        for text in ["a".repeat(121),"é".repeat(61),"\u{7f}".into(),"\n".into()]{value["values"][0]["value"]=json!(text);assert!(compact_feedback(&value,&document).is_err());}
        let native=frame(false);
        for (key,bad) in [("ok",json!(false)),("type",json!("other")),("ackStage",json!("submitted")),("submitted",json!(true))]{
            let mut reply=ack(&native);reply[key]=bad;assert!(attach_render_feedback(&mut reply,&native,&source(),STEADY_ACK_BYTES).is_err());
        }
        assert!(compact_feedback(&native,&json!({})).is_err());
    }}
