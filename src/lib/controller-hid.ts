/** Selected receive-only HID diagnostics. Native descriptors and samples stay native-owned. */
export type HidCommand = 'controller_hid_list' | 'controller_hid_inspect' | 'controller_hid_catalog' | 'controller_hid_snapshot'
  | 'controller_hid_open' | 'controller_hid_state' | 'controller_hid_reports' | 'controller_hid_get' | 'controller_hid_close';
export type HidRecord = Record<string, unknown>;
export type HidRequest = {command:HidCommand;interfaceId?:string;generation?:string;contractFingerprint?:string;after?:string};
type Session = {interfaceId:string;generation:string};
export type HidInspectionState = {
  interfaces:HidRecord[];snapshots:HidRecord[];selected:string;cached:boolean;detail:HidRecord|null;
  session:Session|null;ownsSession:boolean;current:HidRecord|null;reports:HidRecord|null;busy:boolean;error:string;
};
const record=(value:unknown):value is HidRecord=>!!value&&typeof value==='object'&&!Array.isArray(value);
const message=(reason:unknown)=>reason instanceof Error?reason.message:String(reason);
const initial=():HidInspectionState=>({interfaces:[],snapshots:[],selected:'',cached:false,detail:null,session:null,ownsSession:false,current:null,reports:null,busy:false,error:''});

export class HidInspectionController {
  state=initial();
  private epoch=0;
  private pending=0;
  private disposed=false;
  private after='0';
  constructor(private readonly request:(request:HidRequest)=>Promise<unknown>,private readonly changed:(state:HidInspectionState)=>void=()=>{}){}
  private update(changes:Partial<HidInspectionState>){this.state={...this.state,...changes};this.changed(this.state);}
  private async run(work:()=>Promise<void>,epoch=this.epoch){
    this.pending++;this.update({busy:true,error:''});
    try{await work();}catch(reason){if(epoch===this.epoch&&!this.disposed)this.update({error:message(reason)});}
    finally{this.pending--;this.update({busy:this.pending>0});}
  }
  private async response(request:HidRequest):Promise<HidRecord>{
    const result=await this.request(request);
    if(!record(result)||request.interfaceId&&result.interfaceId!==request.interfaceId
      ||request.generation&&result.generation!==request.generation)throw Error('HID reply does not match the selected interface and generation.');
    return result;
  }
  async refresh(){
    const epoch=this.epoch;
    await this.run(async()=>{
      const inventory=await this.response({command:'controller_hid_list'});
      const catalog=await this.response({command:'controller_hid_catalog'});
      if(epoch!==this.epoch||this.disposed)return;
      if(!Array.isArray(inventory.interfaces)||!inventory.interfaces.every(record)||!Array.isArray(catalog.snapshots)||!catalog.snapshots.every(record))throw Error('HID inventory is unavailable.');
      this.update({interfaces:inventory.interfaces,snapshots:catalog.snapshots});
    });
  }
  async select(interfaceId:string,cached=false){
    const epoch=++this.epoch,previous=this.state.ownsSession?this.state.session:null;
    this.after='0';
    this.update({selected:interfaceId,cached,session:null,ownsSession:false,detail:null,current:null,reports:null,error:''});
    await this.run(async()=>{
      if(previous)await this.response({command:'controller_hid_close',...previous});
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
    if(!interfaceId||cached||!detail||session||this.pending||this.disposed)return;
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
      const reports=await this.response({command:'controller_hid_reports',...session,after:this.after});
      if(epoch===this.epoch&&!this.disposed){
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
  async dispose(){
    this.after='0';
    this.disposed=true;const session=this.state.ownsSession?this.state.session:null;++this.epoch;this.update({...initial(),busy:this.pending>0});
    if(session)try{await this.response({command:'controller_hid_close',...session});}catch{/* A retiring native generation is never reopened automatically. */}
  }
}
