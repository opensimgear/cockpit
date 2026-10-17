import type {DeepReadonly} from './profiles';

export type RetiredJournalRef={id:{kind:'manual'}|{kind:'companion';file:string};owner:{session:string;generation:string};recordHash:string};
export type RetiredProvisionRef={operation:string;recordHash:string;target:number;refreshUncertain:boolean};
export type RetiredRecoveryStatus={required:boolean;outputs:{reference:RetiredJournalRef;phase:'intent'|'confirmed'|'releasing';slots:number[]}[];provisioning:RetiredProvisionRef[];job:null|{jobId:string;operation:string;active:boolean;state:'authorizing'|'running'|'cancelling'|'complete'|'attention';message:string};diagnostics:string[];outputQualified:boolean;provisioningQualified:boolean};
export type RetiredRecoveryRequest={command:'profile_retired_recovery_status'}|{command:'profile_retired_recovery_recover'|'profile_retired_recovery_acknowledge';reference:RetiredJournalRef}|{command:'profile_retired_recovery_provision_recover';reference:RetiredProvisionRef}|{command:'profile_retired_recovery_provision_cancel';jobId:string};
export type RetiredRecoveryCommand=RetiredRecoveryRequest['command'];
const object=(v:unknown):v is Record<string,unknown>=>v!==null&&typeof v==='object'&&!Array.isArray(v);
const exact=(v:unknown,keys:string[]):v is Record<string,unknown>=>object(v)&&Object.keys(v).length===keys.length&&keys.every(k=>Object.hasOwn(v,k));
const uuid=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(v);
const hash=(v:unknown):v is string=>typeof v==='string'&&/^sha256:[a-f0-9]{64}$/.test(v);
const counter=(v:unknown):v is string=>typeof v==='string'&&/^[1-9][0-9]{0,19}$/.test(v)&&BigInt(v)<=18446744073709551615n;
const message=(v:unknown):v is string=>typeof v==='string'&&[...v].length<=512;
const slot=(v:unknown):v is number=>Number.isInteger(v)&&Number(v)>=1&&Number(v)<=16;
const journal=(v:unknown):v is RetiredJournalRef=>exact(v,['id','owner','recordHash'])&&object(v.id)
 &&(exact(v.id,['kind'])&&v.id.kind==='manual'||exact(v.id,['kind','file'])&&v.id.kind==='companion'&&typeof v.id.file==='string'&&/^instance-[a-f0-9]{32}$/.test(v.id.file))
 &&exact(v.owner,['session','generation'])&&uuid(v.owner.session)&&counter(v.owner.generation)&&hash(v.recordHash);
const provision=(v:unknown):v is RetiredProvisionRef=>exact(v,['operation','recordHash','target','refreshUncertain'])&&uuid(v.operation)&&hash(v.recordHash)&&slot(v.target)&&typeof v.refreshUncertain==='boolean';
const job=(v:unknown):boolean=>v===null||exact(v,['jobId','operation','active','state','message'])&&uuid(v.jobId)&&uuid(v.operation)&&typeof v.active==='boolean'&&['authorizing','running','cancelling','complete','attention'].includes(String(v.state))&&message(v.message);
function capture<T>(value:T):DeepReadonly<T>{const copy=JSON.parse(JSON.stringify(value)) as T;const freeze=(v:unknown)=>{if(v&&typeof v==='object'){Object.values(v).forEach(freeze);Object.freeze(v);}};freeze(copy);return copy as DeepReadonly<T>;}
export function retiredRecoveryStatus(value:unknown):DeepReadonly<RetiredRecoveryStatus>{
 if(!exact(value,['required','outputs','provisioning','job','diagnostics','outputQualified','provisioningQualified'])||typeof value.required!=='boolean'||typeof value.outputQualified!=='boolean'||typeof value.provisioningQualified!=='boolean'
  ||!Array.isArray(value.outputs)||value.outputs.length>33||!value.outputs.every(v=>exact(v,['reference','phase','slots'])&&journal(v.reference)&&['intent','confirmed','releasing'].includes(String(v.phase))&&Array.isArray(v.slots)&&v.slots.length>=1&&v.slots.length<=8&&v.slots.every(slot)&&new Set(v.slots).size===v.slots.length)
  ||new Set(value.outputs.map(v=>JSON.stringify(v.reference.id))).size!==value.outputs.length
  ||!Array.isArray(value.provisioning)||value.provisioning.length>16||!value.provisioning.every(provision)||new Set(value.provisioning.map(v=>v.operation)).size!==value.provisioning.length
  ||!job(value.job)||!Array.isArray(value.diagnostics)||value.diagnostics.length>2||!value.diagnostics.every(message))throw new Error('Retained recovery status is unavailable or malformed. Refresh its native records.');
 return capture(value as RetiredRecoveryStatus);
}
export function needsRetiredRecovery(status:DeepReadonly<RetiredRecoveryStatus>|null):boolean{return !!status&&(status.required||status.outputs.length>0||status.provisioning.length>0||!!status.job?.active||status.job?.state==='attention'||status.diagnostics.length>0);}
export class RetiredRecoveryClient{
 constructor(private send:(request:RetiredRecoveryRequest)=>Promise<unknown>){}
 private async request(request:RetiredRecoveryRequest){return retiredRecoveryStatus(await this.send(capture(request) as RetiredRecoveryRequest));}
 status(){return this.request({command:'profile_retired_recovery_status'});}
 recover(reference:DeepReadonly<RetiredJournalRef>){if(!journal(reference))throw new Error('Choose an exact retained output record.');return this.request({command:'profile_retired_recovery_recover',reference});}
 acknowledge(reference:DeepReadonly<RetiredJournalRef>){if(!journal(reference))throw new Error('Choose an exact retained output record.');return this.request({command:'profile_retired_recovery_acknowledge',reference});}
 recoverProvision(reference:DeepReadonly<RetiredProvisionRef>){if(!provision(reference))throw new Error('Choose an exact retained provisioning record.');return this.request({command:'profile_retired_recovery_provision_recover',reference});}
 cancel(jobId:string){if(!uuid(jobId))throw new Error('Choose the current native cleanup job.');return this.request({command:'profile_retired_recovery_provision_cancel',jobId});}
}
export type RetiredRecoveryView={status:DeepReadonly<RetiredRecoveryStatus>|null;loading:boolean;stale:boolean;pending:'recover'|'acknowledge'|'provision-recover'|'cancel'|null;error:string};
type Client=Pick<RetiredRecoveryClient,'status'|'recover'|'acknowledge'|'recoverProvision'|'cancel'>;
const same=(a:unknown,b:unknown)=>JSON.stringify(a)===JSON.stringify(b);
/** UI guards only. Native compares the immutable journal hash and exact owner again. */
export class RetiredRecoveryController{
 private value:RetiredRecoveryView={status:null,loading:false,stale:true,pending:null,error:''};private epoch=0;private reading=false;private disposed=false;private listeners=new Set<(view:DeepReadonly<RetiredRecoveryView>)=>void>();
 constructor(private client:Client){}
 get state():DeepReadonly<RetiredRecoveryView>{return Object.freeze(this.value);}
 subscribe(fn:(view:DeepReadonly<RetiredRecoveryView>)=>void){this.listeners.add(fn);fn(this.state);return()=>this.listeners.delete(fn);}
 private set(value:Partial<RetiredRecoveryView>){if(this.disposed)return;this.value={...this.value,...value};for(const fn of this.listeners)fn(this.state);}
 async refresh(){if(this.disposed||this.reading||this.value.pending)return false;const epoch=this.epoch;this.reading=true;this.set({loading:true});try{const status=retiredRecoveryStatus(await this.client.status());if(this.disposed||epoch!==this.epoch)return false;this.set({status,stale:false,error:''});return true;}catch(error){if(epoch===this.epoch)this.set({stale:true,error:String(error instanceof Error?error.message:error)});return false;}finally{this.reading=false;if(epoch===this.epoch)this.set({loading:false});}}
 private reject(message:string){this.set({error:message});return Promise.resolve(false);}
 output(reference:DeepReadonly<RetiredJournalRef>,acknowledge=false){const status=this.value.status,record=status?.outputs.find(v=>same(v.reference,reference));if(this.value.stale||!record)return this.reject('Refresh and choose the current exact retained output record.');if(!status?.outputQualified)return this.reject('Native output recovery is unavailable until its platform qualification is complete.');if((record.phase==='confirmed')===acknowledge)return this.reject('Confirmed output records require recovery; uncertain records require an explicit no-output acknowledgement.');return this.run(acknowledge?'acknowledge':'recover',()=>acknowledge?this.client.acknowledge(record.reference):this.client.recover(record.reference));}
 recoverProvision(reference:DeepReadonly<RetiredProvisionRef>){const status=this.value.status,record=status?.provisioning.find(v=>same(v,reference));if(this.value.stale||!record)return this.reject('Refresh and choose the current exact provisioning record.');if(!status?.provisioningQualified)return this.reject('Native provisioning recovery is unavailable until its platform qualification is complete.');return this.run('provision-recover',()=>this.client.recoverProvision(record));}
 cancel(){const job=this.value.status?.job;if(!job?.active)return this.reject('No current native cleanup job can be cancelled.');return this.run('cancel',()=>this.client.cancel(job.jobId));}
 private async run(action:NonNullable<RetiredRecoveryView['pending']>,request:()=>Promise<DeepReadonly<RetiredRecoveryStatus>>){if(this.disposed||this.value.pending&&(action!=='cancel'||this.value.pending==='cancel'))return false;const epoch=++this.epoch;this.set({pending:action,loading:false,stale:true,error:''});try{const status=retiredRecoveryStatus(await request());if(this.disposed||epoch!==this.epoch)return false;this.set({status,stale:false});return true;}catch(error){if(epoch===this.epoch)this.set({error:String(error instanceof Error?error.message:error)});return false;}finally{if(epoch===this.epoch)this.set({pending:null});}}
 dispose(){this.disposed=true;this.epoch++;this.listeners.clear();}
}
