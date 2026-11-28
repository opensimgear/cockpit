/** Selected receive-only HID diagnostics. Native descriptors and samples stay native-owned. */
import {exportChunk} from './hid-inspector/download';
export type HidCommand = 'controller_hid_list' | 'controller_hid_inspect' | 'controller_hid_catalog' | 'controller_hid_snapshot'
  | 'controller_hid_open' | 'controller_hid_state' | 'controller_hid_reports' | 'controller_hid_get' | 'controller_hid_close'
  | 'controller_hid_record_list'|'controller_hid_record_start'|'controller_hid_record_stop'|'controller_hid_record_status'|'controller_hid_record_export'|'controller_hid_record_discard'
  | 'controller_hid_replay_import'|'controller_hid_replay_read'|'controller_hid_replay_close';
export type HidRecord = Record<string, unknown>;
export type HidRequest = {command:HidCommand;interfaceId?:string;generation?:string;contractFingerprint?:string;after?:string;
  requestId?:string;kind?:'input'|'feature';reportId?:number;maxBytes?:number;deadlineMs?:number;maxRecords?:number;
  recording?:string;replayId?:string;elapsedUs?:string;limit?:number;offset?:number;seek?:boolean};
type Session = {interfaceId:string;generation:string};
type Retired = Session & {status:'disconnected'|'replaced'|'ended';reason:unknown};
export type HidInspectionState = {
  interfaces:HidRecord[];snapshots:HidRecord[];selected:string;cached:boolean;detail:HidRecord|null;
  session:Session|null;retired:Retired|null;ownsSession:boolean;current:HidRecord|null;reports:HidRecord|null;busy:boolean;error:string;
  getResult:HidRecord|null;recording:HidRecord|null;recordingTarget:Session|null;recordings:HidRecord[];replay:HidRecord|null;
};
const record=(value:unknown):value is HidRecord=>!!value&&typeof value==='object'&&!Array.isArray(value);
const message=(reason:unknown)=>record(reason)&&typeof reason.message==='string'?reason.message:String(reason);
const initial=():HidInspectionState=>({interfaces:[],snapshots:[],selected:'',cached:false,detail:null,session:null,retired:null,ownsSession:false,current:null,reports:null,busy:false,error:'',getResult:null,recording:null,recordingTarget:null,recordings:[],replay:null});

export class HidInspectionController {
  state=initial();
  private epoch=0;
  private pending=0;
  private disposed=false;
  private after='0';
  constructor(private readonly request:(request:HidRequest)=>Promise<unknown>,private readonly changed:(state:HidInspectionState)=>void=()=>{}){}
  private update(changes:Partial<HidInspectionState>){this.state={...this.state,...changes};this.changed(this.state);}
  private retire(session:Session,status:Retired['status'],reason:unknown){
    if(this.state.session?.interfaceId!==session.interfaceId||this.state.session?.generation!==session.generation)return;
    ++this.epoch;this.after='0';
    const recording=this.state.recordingTarget?.interfaceId===session.interfaceId&&this.state.recordingTarget.generation===session.generation&&record(this.state.recording?.result)&&this.state.recording.result.status==='recording'
      ?{...this.state.recording,result:{...this.state.recording.result,status:'unavailable',reason:'Capture retired; check retained archive status.'}}:this.state.recording;
    this.update({session:null,ownsSession:false,current:null,reports:null,getResult:null,retired:{...session,status,reason},recording,error:''});
  }
  private async run(work:()=>Promise<void>,epoch=this.epoch){
    this.pending++;this.update({busy:true,error:''});
    try{await work();}catch(reason){if(epoch===this.epoch&&!this.disposed)this.update({error:message(reason)});}
    finally{this.pending--;this.update({busy:this.pending>0});}
  }
  private async response(request:HidRequest):Promise<HidRecord>{
    let result:unknown;
    try{result=await this.request(request);}catch(reason){
      const text=message(reason);
      if(request.interfaceId&&request.generation&&/^(?:Stale HID generation|Offline: no open HID session)(?:$|[.:])/i.test(text)){
        this.retire({interfaceId:request.interfaceId,generation:request.generation},/^stale/i.test(text)?'replaced':'disconnected',text);
      }
      throw reason;
    }
    if(!this.disposed&&record(result)&&request.interfaceId&&request.generation&&this.state.session?.interfaceId===request.interfaceId&&this.state.session.generation===request.generation
      &&result.interfaceId===request.interfaceId&&typeof result.generation==='string'&&result.generation!==request.generation){
      this.retire({interfaceId:request.interfaceId,generation:request.generation},'replaced','Native reply belongs to a different capture generation.');
      this.update({error:'HID reply does not match the selected interface and generation.'});
    }
    if(!record(result)||request.interfaceId&&result.interfaceId!==request.interfaceId
      ||request.generation&&result.generation!==request.generation||request.replayId&&result.replayId!==request.replayId)throw Error('HID reply does not match the selected interface and generation or replay.');
    return result;
  }
  async refresh(){
    const epoch=this.epoch;
    await this.run(async()=>{
      const inventory=await this.response({command:'controller_hid_list'});
      const catalog=await this.response({command:'controller_hid_catalog'});
      const archives=await this.response({command:'controller_hid_record_list'});
      if(epoch!==this.epoch||this.disposed)return;
      if(!Array.isArray(inventory.interfaces)||!inventory.interfaces.every(record)||!Array.isArray(catalog.snapshots)||!catalog.snapshots.every(record))throw Error('HID inventory is unavailable.');
      const recordings=Array.isArray(archives.recordings)?archives.recordings.filter(record):[];
      const target=this.state.recordingTarget,retained=target?recordings.find(row=>row.interfaceId===target.interfaceId&&row.generation===target.generation):undefined;
      this.update({interfaces:inventory.interfaces,snapshots:catalog.snapshots,recordings,...(retained?{recording:{...target,result:retained}}:{})});
      const session=this.state.session;
      if(session){
        const row=inventory.interfaces.find(row=>row.interfaceId===session.interfaceId);
        const complete=inventory.truncated!==true&&(!Array.isArray(inventory.failures)||inventory.failures.length===0);
        const replaced=row&&(typeof row.captureGeneration==='string'&&row.captureGeneration!==session.generation
          ||typeof row.contractFingerprint==='string'&&typeof this.state.detail?.contractFingerprint==='string'&&row.contractFingerprint!==this.state.detail.contractFingerprint);
        if(replaced||row&&row.captureGeneration!==session.generation||!row&&complete)this.retire(session,replaced?'replaced':'disconnected',
          replaced?'Inventory reports a replacement capture generation or contract.':'Refreshed inventory no longer reports this capture generation.');
      }
    });
  }
  async select(interfaceId:string,cached=false){
    const epoch=++this.epoch,previous=this.state.ownsSession?this.state.session:null;
    const replay=this.state.replay;
    this.after='0';
    this.update({selected:interfaceId,cached,session:null,retired:null,ownsSession:false,detail:null,current:null,reports:null,error:'',getResult:null,replay:null});
    await this.run(async()=>{
      if(previous)await this.response({command:'controller_hid_close',...previous});
      if(typeof replay?.replayId==='string')await this.response({command:'controller_hid_replay_close',replayId:replay.replayId});
      if(!interfaceId||this.disposed||epoch!==this.epoch)return;
      const detail=await this.response({command:cached?'controller_hid_snapshot':'controller_hid_inspect',interfaceId});
      if(epoch===this.epoch&&!this.disposed){
        const row=this.state.interfaces.find(row=>row.interfaceId===interfaceId);
        const shared=!cached&&typeof row?.captureGeneration==='string'&&row.contractFingerprint===detail.contractFingerprint
          ?{interfaceId,generation:row.captureGeneration}:null;
        this.update({detail,session:shared});
      }
    },epoch);
  }
  async open(){
    const {selected:interfaceId,cached,detail,session}=this.state,epoch=this.epoch;
    if(!interfaceId||cached||!detail||session||this.state.retired||this.pending||this.disposed)return;
    await this.run(async()=>{
      const opened=await this.response({command:'controller_hid_open',interfaceId,
        ...(typeof detail.contractFingerprint==='string'?{contractFingerprint:detail.contractFingerprint}:{})});
      if(typeof opened.generation!=='string'||!opened.generation)throw Error('HID capture generation is unavailable.');
      const selected={interfaceId,generation:opened.generation};
      if(epoch!==this.epoch||this.disposed){await this.response({command:'controller_hid_close',...selected});return;}
      this.after='0';this.update({session:selected,ownsSession:true});
    },epoch);
  }
  async read(){
    const session=this.state.session,epoch=this.epoch;
    if(!session||this.pending||this.disposed)return;
    this.update({current:null,reports:null});
    await this.run(async()=>{
      const current=await this.response({command:'controller_hid_state',...session});
      if(epoch!==this.epoch||this.disposed)return;
      const state=record(current.result)?current.result:{};
      if(state.terminal||state.status==='offline'){
        this.retire(session,'ended',state.terminal??{kind:'offline'});
        if(this.state.recordingTarget?.interfaceId===session.interfaceId&&this.state.recordingTarget.generation===session.generation){
          try{
            const recording=await this.response({command:'controller_hid_record_status',...session});
            if(!this.disposed&&this.state.recordingTarget?.interfaceId===session.interfaceId&&this.state.recordingTarget.generation===session.generation)this.update({recording});
          }catch{/* Retained archive status is independently refreshable after a terminal capture. */}
        }
        return;
      }
      const reports=await this.response({command:'controller_hid_reports',...session,after:this.after});
      if(epoch===this.epoch&&!this.disposed){
        if(record(reports.result)&&reports.result.terminal){this.retire(session,'ended',reports.result.terminal);return;}
        const rows=record(reports.result)&&Array.isArray(reports.result.reports)?reports.result.reports:[];
        const last=rows.at(-1);
        if(record(last)&&typeof last.sequence==='string'&&/^(0|[1-9][0-9]*)$/.test(last.sequence))this.after=last.sequence;
        this.update({current,reports});
      }
    },epoch);
  }
  async close(){
    const session=this.state.ownsSession?this.state.session:null;
    this.after='0';
    ++this.epoch;this.update({session:null,ownsSession:false,current:null,reports:null});
    if(session)await this.run(async()=>{await this.response({command:'controller_hid_close',...session});});
  }
  async get(kind:'input'|'feature',reportId:number,maxBytes:number,deadlineMs:number){
    const session=this.state.session,epoch=this.epoch;
    if(!session||this.pending||this.disposed)return;
    await this.run(async()=>{
      const getResult=await this.response({command:'controller_hid_get',...session,requestId:crypto.randomUUID(),kind,reportId,maxBytes,deadlineMs});
      if(epoch===this.epoch&&!this.disposed)this.update({getResult});
    },epoch);
  }
  async record(action:'start'|'stop'|'status'|'discard',maxBytes=8*1024*1024,maxRecords=10000){
    const session=action==='start'?this.state.session:this.state.recordingTarget??this.state.session,epoch=this.epoch;
    if(!session||this.pending||this.disposed)return;
    await this.run(async()=>{
      const recording=await this.response({command:`controller_hid_record_${action}`,...session,...(action==='start'?{maxBytes,maxRecords}:{})});
      if(epoch===this.epoch&&!this.disposed)this.update({recording:action==='discard'?null:recording,recordingTarget:action==='discard'?null:session});
    },epoch);
  }
  async selectRecording(interfaceId:string,generation:string){
    if(this.pending||this.disposed)return;
    this.update({recordingTarget:{interfaceId,generation},recording:null});await this.record('status');
  }
  async exportRecording():Promise<Uint8Array<ArrayBuffer>[]|null>{
    const session=this.state.recordingTarget??this.state.session,epoch=this.epoch;let chunks:Uint8Array<ArrayBuffer>[]|null=null;
    if(!session||this.pending||this.disposed)return null;
    await this.run(async()=>{
      let offset=0,total:number|null=null;const parts:Uint8Array<ArrayBuffer>[]=[];
      do{
        const result=await this.response({command:'controller_hid_record_export',...session,offset,limit:32768});
        if(epoch!==this.epoch||this.disposed)return;
        const page=exportChunk(record(result.result)?result.result:{},offset,total);
        parts.push(page.bytes);offset=page.next;total=page.total;
      }while(offset<total);
      chunks=parts;
    },epoch);return chunks;
  }
  async importRecording(text:string){
    if(this.pending||this.disposed)return;
    if(new TextEncoder().encode(text).byteLength>64*1024*1024){this.update({error:'Recording exceeds the 64 MiB import limit.'});return;}
    const epoch=this.epoch;
    await this.run(async()=>{
      const replay=await this.response({command:'controller_hid_replay_import',recording:text});
      if(typeof replay.replayId!=='string')throw Error('Replay identity is unavailable.');
      if(epoch!==this.epoch||this.disposed){await this.response({command:'controller_hid_replay_close',replayId:replay.replayId});return;}
      await this.select('');
      if(this.epoch!==epoch+1||this.disposed||this.state.error){await this.response({command:'controller_hid_replay_close',replayId:replay.replayId});return;}
      this.update({replay,detail:record(replay.descriptor)?replay.descriptor:null});
    },epoch);
  }
  async replayRead(after='0',elapsedUs?:string,limit=128,seek=false){
    const replay=this.state.replay,epoch=this.epoch;
    if(typeof replay?.replayId!=='string'||this.pending||this.disposed)return;
    const replayId=replay.replayId;
    await this.run(async()=>{
      let page:HidRecord,entries:unknown[]=[];
      do{
        page=await this.response({command:'controller_hid_replay_read',replayId,after,limit,...(elapsedUs===undefined?{}:{elapsedUs}),...(seek?{seek:true}:{})});
        if(epoch!==this.epoch||this.disposed)return;
        if(Array.isArray(page.records))entries=[...entries,...page.records].slice(-256);
        if(typeof page.next!=='string'||page.next===after)break;
        after=page.next;
      }while(elapsedUs!==undefined&&page.hasMore===true);
      if(epoch===this.epoch&&!this.disposed)this.update({replay:{...replay,...page},current:record(page.state)?{result:page.state}:null,
        reports:{result:{reports:entries,gap:false}}});
    },epoch);
  }
  async dispose(){
    this.after='0';
    this.disposed=true;const session=this.state.ownsSession?this.state.session:null,replay=this.state.replay;++this.epoch;this.update({...initial(),busy:this.pending>0});
    if(session)try{await this.response({command:'controller_hid_close',...session});}catch{/* A retiring native generation is never reopened automatically. */}
    if(typeof replay?.replayId==='string')try{await this.response({command:'controller_hid_replay_close',replayId:replay.replayId});}catch{/* Offline observations can be safely retired. */}
  }
}
