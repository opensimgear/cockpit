use super::*;
use serde_json::{json, Value};
use std::time::Instant;

fn state(generation:u32,saved_generation:u32,owner:u32,claim_expires_ms:u32,dirty:bool,pending_apply:bool)->Value{
    json!({"generation":generation,"savedGeneration":saved_generation,"owner":owner,
        "claimExpiresMs":claim_expires_ms,"dirty":dirty,"pendingApply":pending_apply})
}
fn line(boot:u32,sequence:u32,kind:&str,revision:u32,exhausted:bool,state:Value)->String{
    format!("@meta {}",json!({"boot":boot,"sequence":sequence,"kind":kind,"revision":revision,
        "exhausted":exhausted,"state":state}))
}
fn baseline(state:Value)->String{line(7,1,"baseline",1,false,state)}
fn change(sequence:u32,revision:u32,exhausted:bool,state:Value)->String{
    line(7,sequence,"change",revision,exhausted,state)
}
fn clean()->Value{state(4,3,0,0,false,false)}

#[test]
fn baseline_change_and_native_receive_timestamp_are_preserved(){
    let mut decoder=Decoder::new(7);let at=Instant::now();
    let first=decoder.decode(&baseline(clean()),at).unwrap();
    assert_eq!(first.kind,Kind::Baseline);assert_eq!(first.received_at,at);
    assert_eq!(first.state.generation,4);assert_eq!(first.state.saved_generation,3);
    let next=state(5,4,12,0,true,false);
    let at=Instant::now();let frame=decoder.decode(&change(2,8,false,next),at).unwrap();
    assert_eq!(frame.kind,Kind::Change);assert_eq!(frame.revision,8);assert_eq!(frame.received_at,at);
    assert_eq!(frame.state.owner,12);assert_eq!(frame.state.claim_expires_ms,0);
    let encoded=serde_json::to_value(&frame.state).unwrap();
    assert_eq!(encoded["savedGeneration"],4);assert_eq!(encoded["claimExpiresMs"],0);
}

#[test]
fn expiry_may_wrap_and_owner_zero_requires_zero_expiry(){
    let mut decoder=Decoder::new(7);
    decoder.decode(&baseline(state(1,1,9,u32::MAX,false,false)),Instant::now()).unwrap();
    let frame=decoder.decode(&change(2,2,false,state(1,1,9,0,false,false)),Instant::now()).unwrap();
    assert_eq!(frame.state.claim_expires_ms,0);
    assert!(Decoder::new(7).decode(&baseline(state(1,1,0,1,false,false)),Instant::now()).is_err());
    assert!(Decoder::new(7).decode(&baseline(state(1,1,1,u32::MAX,false,false)),Instant::now()).is_ok());
}

#[test]
fn terminal_exhaustion_can_reuse_revision_once_and_is_sticky(){
    let mut decoder=Decoder::new(7);
    let max_state=state(u32::MAX,u32::MAX,u32::MAX,0,false,false);
    let max_baseline=line(7,1,"baseline",u32::MAX,false,max_state.clone());
    assert!(max_baseline.len()<=MAX_LINE_BYTES);
    decoder.decode(&max_baseline,Instant::now()).unwrap();
    let terminal=decoder.decode(&change(2,u32::MAX,true,state(u32::MAX,u32::MAX,u32::MAX,0,false,false)),Instant::now()).unwrap();
    assert!(terminal.exhausted);assert_eq!(terminal.revision,u32::MAX);
    assert!(decoder.decode(&change(3,u32::MAX,true,state(u32::MAX,u32::MAX,u32::MAX,0,false,false)),Instant::now()).is_err());
    let mut terminal_baseline=Decoder::new(7);
    assert!(terminal_baseline.decode(&line(7,1,"baseline",1,true,clean()),Instant::now()).unwrap().exhausted);
    assert!(terminal_baseline.decode(&change(2,2,false,clean()),Instant::now()).is_err());
}

#[test]
fn rejected_frames_do_not_advance_sequence_revision_or_configuration_counters(){
    let mut decoder=Decoder::new(7);decoder.decode(&baseline(state(2,2,0,0,false,false)),Instant::now()).unwrap();
    assert!(decoder.decode(&change(3,3,false,state(3,3,0,0,false,false)),Instant::now()).is_err());
    assert!(decoder.decode(&change(2,1,false,state(1,1,0,0,false,false)),Instant::now()).is_err());
    assert!(decoder.decode(&change(2,3,false,state(1,2,0,0,false,true)),Instant::now()).is_err());
    let accepted=decoder.decode(&change(2,3,false,state(3,3,0,0,false,false)),Instant::now()).unwrap();
    assert_eq!(accepted.state.generation,3);
}

#[test]
fn same_revision_is_allowed_only_for_terminal_exhaustion(){
    let mut decoder=Decoder::new(7);decoder.decode(&baseline(clean()),Instant::now()).unwrap();
    assert!(decoder.decode(&change(2,1,false,clean()),Instant::now()).is_err());
    assert!(decoder.decode(&change(2,1,true,clean()),Instant::now()).unwrap().exhausted);
}

#[test]
fn pending_apply_allows_only_one_checked_generation_ahead(){
    let valid=state(10,11,4,500,true,true);
    assert!(Decoder::new(7).decode(&baseline(valid),Instant::now()).is_ok());
    assert!(Decoder::new(7).decode(&baseline(state(10,12,4,500,true,true)),Instant::now()).is_err());
    assert!(Decoder::new(7).decode(&baseline(state(u32::MAX,u32::MAX,4,0,false,true)),Instant::now()).is_ok());
    assert!(Decoder::new(7).decode(&baseline(state(u32::MAX,u32::MAX,4,0,false,false)),Instant::now()).is_ok());
}

#[test]
fn wrong_boot_counter_gap_and_sequence_wrap_are_rejected(){
    assert!(Decoder::new(8).decode(&baseline(clean()),Instant::now()).is_err());
    assert!(Decoder::new(u32::MAX).decode(&line(u32::MAX,1,"baseline",u32::MAX,false,
        state(u32::MAX,u32::MAX,u32::MAX,0,false,false)),Instant::now()).is_ok());
    let mut decoder=Decoder::new(7);decoder.decode(&baseline(clean()),Instant::now()).unwrap();
    assert!(decoder.decode(&change(3,2,false,clean()),Instant::now()).is_err());
    decoder.position=Some(Position{sequence:u32::MAX,revision:2,generation:4,saved_generation:3,exhausted:false});
    assert!(decoder.decode(&change(1,3,false,clean()),Instant::now()).is_err());
    decoder.position=Some(Position{sequence:u32::MAX-1,revision:2,generation:4,saved_generation:3,exhausted:false});
    assert!(decoder.decode(&change(u32::MAX,3,false,clean()),Instant::now()).is_err());
    assert!(decoder.decode(&change(u32::MAX,3,true,clean()),Instant::now()).unwrap().exhausted);
}

#[test]
fn exact_schema_rejects_duplicates_extras_nulls_controls_and_oversize(){
    let mut decoder=Decoder::new(7);
    let duplicate_top=r#"@meta {"boot":7,"boot":7,"sequence":1,"kind":"baseline","revision":1,"exhausted":false,"state":{"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":false,"pendingApply":false}}"#;
    let duplicate_state=r#"@meta {"boot":7,"sequence":1,"kind":"baseline","revision":1,"exhausted":false,"state":{"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":false,"dirty":false,"pendingApply":false}}"#;
    let extra_top=r#"@meta {"boot":7,"sequence":1,"kind":"baseline","revision":1,"exhausted":false,"extra":0,"state":{"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":false,"pendingApply":false}}"#;
    let extra_state=r#"@meta {"boot":7,"sequence":1,"kind":"baseline","revision":1,"exhausted":false,"state":{"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":false,"pendingApply":false,"extra":0}}"#;
    let null_field=r#"@meta {"boot":7,"sequence":1,"kind":"baseline","revision":1,"exhausted":false,"state":{"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":null,"pendingApply":false}}"#;
    let missing_top=r#"@meta {"boot":7,"sequence":1,"kind":"baseline","revision":1,"state":{"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":false,"pendingApply":false}}"#;
    let missing_state=r#"@meta {"boot":7,"sequence":1,"kind":"baseline","revision":1,"exhausted":false,"state":{"generation":1,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":false}}"#;
    for packet in [duplicate_top,duplicate_state,extra_top,extra_state,null_field,missing_top,missing_state]{
        assert!(decoder.decode(packet,Instant::now()).is_err(),"accepted {packet}");
    }
    assert!(decoder.decode(&format!("{}\t",baseline(clean())),Instant::now()).is_err());
    assert!(decoder.decode(&format!("@meta {}", " ".repeat(MAX_LINE_BYTES)),Instant::now()).is_err());
    assert!(decoder.decode("@meta {\"boot\":7}",Instant::now()).is_err());
    assert!(decoder.decode(&baseline(clean()),Instant::now()).is_ok());
}

#[test]
fn compiled_firmware_metadata_vectors_preserve_commits_expiry_and_terminal_exhaustion(){
    let at=Instant::now();let mut decoder=Decoder::new(77);
    let frames=include_str!("../fixtures/module-metadata.jsonl").lines()
        .map(|line|decoder.decode(line,at).unwrap_or_else(|error|panic!("{error}: {line}"))).collect::<Vec<_>>();
    assert_eq!(frames.len(),9);
    for (index,frame) in frames.iter().enumerate(){assert_eq!(frame.sequence,index as u32+1);assert_eq!(frame.received_at,at);}
    assert_eq!(frames.iter().map(|frame|frame.revision).collect::<Vec<_>>(),vec![1,2,3,4,5,6,7,8,u32::MAX]);
    assert_eq!(frames[0].kind,Kind::Baseline);assert_eq!(frames[1].state.owner,42);
    assert!(frames[2].state.dirty);assert_eq!(frames[3].state.owner,0);assert!(!frames[3].state.dirty);
    assert!(frames[6].state.pending_apply&&frames[6].state.dirty);assert_eq!(frames[6].state.generation,1);
    assert!(!frames[7].state.pending_apply&&!frames[7].state.dirty);assert_eq!(frames[7].state.generation,2);
    assert!(frames[8].exhausted);
}
