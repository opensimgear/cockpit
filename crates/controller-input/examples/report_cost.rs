//! Synthetic admission-cost diagnostic, never a physical latency qualification.
use osg_controller_input::registry::{Control,Definition,Identity,Registry};
use osg_streams::Broker;
use serde_json::json;
use std::{collections::BTreeMap,time::Instant};

fn main() {
    let mut results=Vec::new();
    for scenario in ["never-opened","open","opened-then-closed"] {
        let observed=scenario!="never-opened";
        let mut registry=Registry::default();
        let mut controls=Vec::new();
        for index in 0..128 {controls.push(Control::new(9,index+1,1,0,index));}
        for index in 0..6 {
            let mut axis=Control::new(1,0x30+index,1,0,128+index);
            axis.bits=12;axis.max=4095;controls.push(axis);
        }
        let mut hat=Control::new(1,0x39,1,0,134);hat.bits=4;hat.max=7;hat.has_null=true;controls.push(hat);
        for index in 135..137 {let mut raw=Control::new(0,0,1,0,index);raw.bits=12;raw.max=4095;controls.push(raw);}
        let id=registry.connect(Definition {
            identity:Identity {backend:"diagnostic".into(),vendor_id:1,product_id:2,serial:None,
                collection:"capture-cost".into(),fingerprint:String::new(),path:"synthetic".into()},
            name:"Synthetic 137-control report".into(),controls,diagnostics:vec![],
        }).unwrap();
        let mut data=BTreeMap::from([(128,0),(129,2048),(130,2048),(131,2048),(132,2048),(133,2048),(134,8),(135,0),(136,0)]);
        registry.report(&id,1,&data).unwrap();
        let broker=Broker::new("report-cost".into()).unwrap();
        let mut reader=if observed {Some(registry.prepare_observation(&broker,&id).unwrap().with_subscription(||broker.subscribe("diagnostic","ui",&format!("controllers.input.{id}")).map_err(|e|e.to_string())).unwrap())}else{None};
        if let Some(reader)=&mut reader {reader.recv().unwrap();}
        if scenario=="opened-then-closed" {drop(reader.take());}
        let mut times=Vec::new();
        for index in 0..600 {
            data.insert(128,(index*29)%4096);
            let started=Instant::now();
            registry.report(&id,1,&data).unwrap();
            let elapsed=started.elapsed().as_micros() as u64;
            if index>=100 {times.push(elapsed);}
            if let Some(reader)=&mut reader {
                let frame=reader.recv().unwrap();assert_eq!(frame.kind,"change");
                let payload=frame.payload.unwrap();
                assert_eq!(payload["source"]["sequence"],(index+2).to_string());
                assert_eq!(payload["values"]["hid:0001:0030:1:0:128"]["native"],data[&128]);
            }
        }
        times.sort_unstable();
        results.push(json!({"scenario":scenario,"previewRegistered":reader.is_some(),"controls":137,"samples":times.len(),
            "reportCostUs":{"p50":times[times.len()/2],"p95":times[times.len()*95/100],"p99":times[times.len()*99/100],"max":times.last()}}));
    }
    println!("{}",serde_json::to_string_pretty(&results).unwrap());
}
