//! Physical A/B package demonstration through the production Hub and USB manager.
//! Treats the supplied protected state as read-only and runs in its own child.
use base64::{engine::general_purpose::STANDARD, Engine};
use osg_companion_serial::Manager;
use osg_companion_service::{render_packages::RenderPackageUpload, *};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::{Arc, Mutex}, thread, time::{Duration, Instant}};

#[derive(Default)]
struct Evidence(Mutex<Value>);
impl Evidence {
    fn count(&self, key: &str, instance:&str) {
        let mut state=self.0.lock().unwrap();let count=state[key].as_u64().unwrap_or(0)+1;
        if !state.is_object(){*state=json!({});}state[key]=json!(count);
        let count=state["instances"][instance][key].as_u64().unwrap_or(0)+1;
        state["instances"][instance][key]=json!(count);
    }
}
impl SourceSink for Evidence {
    fn register(&self,_:SourceDefinition)->Result<(),String>{Ok(())}
    fn baseline(&self,token:&SourceToken,_:u64,_:&[SourceChange])->Result<(),String>{self.count("baselines",&token.source_id);Ok(())}
    fn changes(&self,token:&SourceToken,_:u64,changes:&[SourceChange])->Result<(),String>{
        self.count("inputBatches",&token.source_id);let mut state=self.0.lock().unwrap();state["lastInputInstance"]=json!(token.source_id);state["lastChanges"]=json!(changes);Ok(())
    }
    fn loss(&self,token:&SourceToken)->Result<(),String>{self.count("sourceLosses",&token.source_id);Ok(())}
}
struct Board { name:String,port:String,instance:String,companion:Option<String>,hash:String,assigned:bool }
fn fixture(board:&str,revision:&str)->Result<(Value,RenderPackageUpload),String>{
    let dir=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/companion-renderer/test/poc").join(board).join(revision);
    let source:Value=serde_json::from_slice(&fs::read(dir.join("source-publication.json")).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let manifest=fs::read_to_string(dir.join("manifest.json")).map_err(|e|e.to_string())?;
    let package_hash=format!("sha256:{:x}",Sha256::digest(manifest.as_bytes()));
    let objects=["semantics","scene","images","fonts"].into_iter().map(|kind|Ok((kind.into(),STANDARD.encode(fs::read(dir.join(format!("{kind}.bin"))).map_err(|e|e.to_string())?)))).collect::<Result<BTreeMap<_,_>,String>>()?;
    // These fixtures use finite, ordinary JSON numbers whose sorted serde encoding
    // is their exact canonical encoding; native publication admission checks it.
    let request=json!({"command":"companion_publish","canonicalDocument":serde_json::to_string(&source["document"]).map_err(|e|e.to_string())?,"publicationHash":source["publicationHash"],"capabilityFingerprint":source["capabilityFingerprint"],"assets":[]});
    Ok((request,RenderPackageUpload{manifest,package_hash,objects}))
}
fn install(hub:&Hub,boards:&mut[Board],revision:&str,reuse:bool)->Result<(),String>{
    for board in boards {
        let(source,package)=fixture(&board.name,revision)?;
        board.hash=hub.command(&source)?["publicationHash"].as_str().ok_or("Publication reply")?.into();
        if !reuse{hub.install_render_package(package.clone())?;}
        board.assigned=false;
        println!("{}",json!({"event":"published","board":board.name,"revision":revision,"publicationHash":board.hash,"packageHash":package.package_hash,"reuse":reuse}));
    }Ok(())
}
fn assign(hub:&Hub,board:&mut Board,companion:&str)->Result<(),String>{
    let status=hub.command(&json!({"command":"companion_status"}))?;
    // A copied host can retain an earlier assignment. Keep exactly one on this
    // bench companion; production credentials and original state are untouched.
    for row in status["instances"].as_array().ok_or("Host instances")? {
        if row["companionId"]==companion&&row["instanceId"]!=board.instance {
            hub.command(&json!({"command":"companion_unassign","instanceId":row["instanceId"]}))?;
        }
    }
    hub.command(&json!({"command":"companion_assign","companionId":companion,"instanceId":board.instance,"publicationHash":board.hash}))?;
    hub.configure_feedback(&board.instance,&board.hash,&[
        FeedbackBinding{channel_id:"reactor-power".into(),output_id:"power-readout".into()},
        FeedbackBinding{channel_id:"laser-confirmed".into(),output_id:"laser-state".into()},
    ])?;
    board.companion=Some(companion.into());board.assigned=true;Ok(())
}
fn indirect(path:&std::path::Path)->Result<bool,String>{
    let metadata=match fs::symlink_metadata(path){Ok(v)=>v,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(false),Err(_)=>return Err("Cannot inspect isolated bench path".into())};
    #[cfg(windows)]
    {use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&0x400!=0{return Ok(true);}}
    Ok(metadata.file_type().is_symlink())
}
fn run()->Result<(),String>{
    let mut args=std::env::args().skip(1);let mut state_dir=None;let mut boards=Vec::new();let mut seconds=180u64;let mut switch_after=0u64;let mut revision="A".to_owned();let mut switch_to="B".to_owned();let mut stop_file=None;let mut reuse=false;
    while let Some(arg)=args.next(){match arg.as_str(){
        "--state-dir"=>state_dir=args.next().map(PathBuf::from),
        "--sunton"|"--waveshare"=>{let name=arg.trim_start_matches("--").to_owned();let port=args.next().ok_or("Missing port")?;boards.push(Board{instance:format!("render-poc-{name}"),name,port,companion:None,hash:String::new(),assigned:false});},
        "--seconds"=>seconds=args.next().ok_or("Missing duration")?.parse().map_err(|_|"Bad duration")?,
        "--switch-after"=>switch_after=args.next().ok_or("Missing switch duration")?.parse().map_err(|_|"Bad switch duration")?,
        "--revision"=>revision=args.next().filter(|r|matches!(r.as_str(),"A"|"B"|"C"|"D")).ok_or("Revision must be A, B, C or D")?,
        "--switch-to"=>switch_to=args.next().filter(|r|matches!(r.as_str(),"A"|"B"|"C"|"D")).ok_or("Switch revision must be A, B, C or D")?,
        "--stop-file"=>stop_file=Some(PathBuf::from(args.next().ok_or("Missing stop file path")?)),
        "--reuse-packages"=>reuse=true,
        _=>return Err("Usage: render-poc --state-dir COPY --sunton COM6 --waveshare COM5 [--revision A|B|C|D] [--seconds 180] [--switch-after 60] [--switch-to B|C|D] [--stop-file PATH] [--reuse-packages]".into()),
    }}
    if switch_after>0&&revision==switch_to{return Err("Choose a different revision for the package switch.".into());}
    if stop_file.as_ref().is_some_and(|path|path.exists()){return Err("Choose a new stop file path before starting the bench.".into());}
    for (index,board) in boards.iter().enumerate(){
        if boards[..index].iter().any(|previous|previous.name==board.name||previous.port.eq_ignore_ascii_case(&board.port)){return Err("Choose each board profile and USB port only once.".into());}
    }
    let directory=state_dir.ok_or("Supply --state-dir containing companion-host-state.dpapi")?;
    let protected_source=directory.join("companion-host-state.dpapi");
    if !protected_source.is_file()||boards.is_empty(){return Err("Choose protected host state and at least one board.".into());}
    let isolated=directory.join("render-poc-run");let destination=isolated.join("companion-host-state.dpapi");
    if indirect(&isolated)?||indirect(&destination)?{return Err("The isolated bench path must not contain links or junctions.".into());}
    fs::create_dir_all(&isolated).map_err(|_|"Cannot prepare isolated bench directory")?;
    fs::copy(&protected_source,&destination).map_err(|_|"Cannot copy protected bench credentials")?;
    // Copy only sealed credentials/assignments. The parent's state is read-only;
    // the child cache survives bench restarts and is always revalidated by Hub.
    let evidence=Arc::new(Evidence::default());let hub=Hub::open(&isolated,evidence.clone())?;install(&hub,&mut boards,&revision,reuse)?;
    let manager=Manager::default();for board in &boards{manager.attach(&board.port,hub.clone())?;}
    let start=Instant::now();let mut last_print=0;let mut switched=false;let mut live_since=None;let mut event_cursors:BTreeMap<String,u64>=BTreeMap::new();
    while start.elapsed().as_secs()<seconds&&!stop_file.as_ref().is_some_and(|path|path.exists()) {
        let rows=manager.statuses();let mut all_live=true;
        for row in &rows{
            let port=row["port"].as_str().unwrap_or("");let cursor=event_cursors.entry(port.into()).or_default();
            for event in row["deviceEvents"].as_array().into_iter().flatten(){
                let sequence=event["sequence"].as_u64().unwrap_or(0);if sequence>*cursor{
                    println!("{}",json!({"event":"device-event","elapsedMs":start.elapsed().as_millis(),"port":port,"sequence":sequence,"transportElapsedMs":event["elapsedMs"],"status":event["status"]}));*cursor=sequence;
                }
            }
        }
        for board in &mut boards {
            let row=rows.iter().find(|row|row["port"]==board.port);
            if let Some(row)=row{
                if !board.assigned {if let Some(id)=row["device"]["companionId"].as_str().filter(|s|!s.is_empty()){assign(&hub,board,id)?;}}
                all_live&=row["connected"]==true&&row["device"]["status"]=="live-armed";
            }else{all_live=false;}
            if let Ok(token)=hub.feedback_token(&board.instance){
                let power=50.+35.*(start.elapsed().as_secs_f64()/4.).sin();
                let samples=[FeedbackSample{channel_id:"reactor-power".into(),output_id:"power-readout".into(),value:Some(FeedbackValue::Number(power)),status:FeedbackStatus::Fresh,provenance:FeedbackProvenance::HostAccepted,source_kind:FeedbackSource::Host,source_age_ms:0,unit:"%".into(),range:Some(FeedbackRange{min:0.,max:100.})},
                    FeedbackSample{channel_id:"laser-confirmed".into(),output_id:"laser-state".into(),value:Some(FeedbackValue::Boolean(start.elapsed().as_secs()%8<4)),status:FeedbackStatus::Fresh,provenance:FeedbackProvenance::HostAccepted,source_kind:FeedbackSource::Host,source_age_ms:0,unit:"state".into(),range:None}];
                // Synthetic bench feedback; these samples claim host acceptance.
                if let Err(error)=hub.publish_feedback(&token,&samples,FeedbackMode::Delta,hub.now_ms()){println!("{}",json!({"event":"feedback-rejected","board":board.name,"reason":error}));}
            }
        }
        if all_live {if live_since.is_none(){live_since=Some(Instant::now());}}else{live_since=None;}
        if !switched&&switch_after>0&&live_since.is_some_and(|at|at.elapsed().as_secs()>=switch_after){
            revision=switch_to.clone();install(&hub,&mut boards,&revision,false)?;switched=true;live_since=None;
        }
        if start.elapsed().as_secs()>=last_print {
            for row in &rows{println!("{}",json!({"event":"device","elapsedMs":start.elapsed().as_millis(),"revision":revision,"port":row["port"],"device":row["device"],"deviceStatus":row["deviceStatus"],"lastWireRequest":row["lastWireRequest"],"lastHostError":row["lastHostError"],"contentBytes":row["renderContentBytes"],"contentRequests":row["renderContentRequests"],"startupAckMaxBytes":row["startupAckMaxBytes"],"steadyAckMaxBytes":row["steadyAckMaxBytes"]}));}
            println!("{}",json!({"event":"input-evidence","elapsedMs":start.elapsed().as_millis(),"counts":evidence.0.lock().unwrap().clone()}));
            last_print=start.elapsed().as_secs()+1;
        }
        thread::sleep(Duration::from_millis(50));
    }
    manager.shutdown();println!("{}",json!({"event":"complete","revision":revision,"switched":switched,"stopReason":if stop_file.as_ref().is_some_and(|path|path.exists()){"operator-stop"}else{"duration"},"counts":evidence.0.lock().unwrap().clone()}));Ok(())
}
fn main(){if let Err(error)=run(){eprintln!("{error}");std::process::exit(1);}}
