import {captureLearningStatus,learningSourcesEqual,learningSourceValid,type ProfileLearningClient,type ProfileLearningSource,type ProfileLearningStatus} from './profile-learning';
import type {ObservedResourcePort,ResourceSummary} from './profile-inventory';
import type {DeepReadonly} from './profiles';
export type ProfileLearningState={source:DeepReadonly<ProfileLearningSource>|null;status:DeepReadonly<ProfileLearningStatus>|null;pending:'start'|'cancel'|null;loading:boolean;stale:boolean;error:string};
type Start={promise:Promise<DeepReadonly<ProfileLearningStatus>>;cleanupOwned:boolean};
/** Native observes movement; the editor must explicitly choose a returned exact candidate. */
export class ProfileLearningController{
  private value:ProfileLearningState={source:null,status:null,pending:null,loading:false,stale:true,error:''};
  private epoch=0;private context='';private disposed=false;private reading=false;private starting:Start|null=null;
  private cancellations=new Map<string,Promise<DeepReadonly<ProfileLearningStatus>>>();
  private listeners=new Set<(state:DeepReadonly<ProfileLearningState>)=>void>();
  constructor(private client:Pick<ProfileLearningClient,'start'|'status'|'cancel'>){}
  get state():DeepReadonly<ProfileLearningState>{return Object.freeze(this.value);}
  subscribe(listener:(state:DeepReadonly<ProfileLearningState>)=>void):()=>void{this.listeners.add(listener);listener(this.state);return()=>this.listeners.delete(listener);}
  private set(next:Partial<ProfileLearningState>):void{if(this.disposed)return;this.value={...this.value,...next};for(const listener of this.listeners)listener(this.state);}
  private cancelNative(id:string):Promise<DeepReadonly<ProfileLearningStatus>>{const pending=this.cancellations.get(id);if(pending)return pending;const request=Promise.resolve().then(()=>this.client.cancel(id));this.cancellations.set(id,request);void request.finally(()=>{if(this.cancellations.get(id)===request)this.cancellations.delete(id);}).catch(()=>{});return request;}
  setContext(context:string):void{if(context!==this.context){this.clear();this.context=context;}}
  clear():void{
    this.epoch++;const status=this.value.status,starting=this.starting;
    if(starting&&!starting.cleanupOwned){starting.cleanupOwned=true;void starting.promise.then(status=>this.cancelNative(status.learnId)).catch(()=>{});}
    else if(status&&status.phase!=='stopped')void this.cancelNative(status.learnId).catch(()=>{});
    this.set({source:null,status:null,pending:null,loading:false,stale:true,error:''});
  }
  async start(source:DeepReadonly<ProfileLearningSource>):Promise<boolean>{
    if(this.disposed||this.value.pending)return false;
    if(!learningSourceValid(source)){this.set({error:'Choose an exact current native input resource.'});return false;}
    const captured=Object.freeze({...source}),epoch=++this.epoch;this.set({source:captured,status:null,pending:'start',loading:false,stale:true,error:''});
    const starting:Start={promise:Promise.resolve().then(()=>this.client.start(captured)),cleanupOwned:false};this.starting=starting;
    try{const status=captureLearningStatus(await starting.promise);if(this.disposed||epoch!==this.epoch){if(!starting.cleanupOwned){starting.cleanupOwned=true;void this.cancelNative(status.learnId).catch(()=>{});}return false;}if(!learningSourcesEqual(captured,status)||status.phase!=='listening')throw new Error('The native learner belongs to another resource generation.');this.set({status,stale:false});return true;}
    catch(error){if(epoch===this.epoch)this.set({error:error instanceof Error?error.message:String(error),stale:true});return false;}
    finally{if(this.starting===starting)this.starting=null;if(epoch===this.epoch)this.set({pending:null});}
  }
  async refresh():Promise<boolean>{
    const status=this.value.status;if(this.disposed||this.reading||this.value.pending||!status||status.phase==='stopped')return false;
    const epoch=this.epoch,source=this.value.source!;this.reading=true;this.set({loading:true});
    try{const next=captureLearningStatus(await this.client.status(status.learnId));if(this.disposed||epoch!==this.epoch)return false;if(next.learnId!==status.learnId||!learningSourcesEqual(source,next)||next.untilUs!==status.untilUs||BigInt(next.capturedUs)<BigInt(status.capturedUs))throw new Error('The input-learning resource capture changed. Restart learning explicitly.');this.set({status:next,stale:false,error:''});return true;}
    catch(error){if(epoch===this.epoch)this.set({stale:true,error:error instanceof Error?error.message:String(error)});return false;}
    finally{this.reading=false;if(epoch===this.epoch)this.set({loading:false});}
  }
  async cancel():Promise<boolean>{
    if(this.disposed||this.value.pending==='cancel')return false;
    const starting=this.starting,status=this.value.status,epoch=++this.epoch;
    if(!starting&&(!status||status.phase==='stopped')){this.clear();return true;}
    if(starting)starting.cleanupOwned=true;this.set({pending:'cancel',loading:false,stale:true,error:''});
    try{const target=starting?await starting.promise:status!,next=captureLearningStatus(await this.cancelNative(target.learnId));if(this.disposed||epoch!==this.epoch)return false;if(next.learnId!==target.learnId||next.phase!=='stopped'||!learningSourcesEqual(target,next))throw new Error('Native input learning did not confirm this capture was cancelled.');this.set({status:next,stale:false});return true;}
    catch(error){if(epoch===this.epoch)this.set({error:error instanceof Error?error.message:String(error),stale:true});return false;}
    finally{if(epoch===this.epoch)this.set({pending:null});}
  }
  async choose(observedPortId:string,contractHash:string,current:DeepReadonly<ResourceSummary>):Promise<DeepReadonly<ObservedResourcePort>>{
    const epoch=this.epoch;
    if(!this.value.status||this.value.status.phase!=='learned'||!await this.refresh()||epoch!==this.epoch)throw new Error('Refresh the current native learned capture before choosing a candidate.');
    const status=this.value.status,source=this.value.source;
    if(this.disposed||this.value.pending||this.value.stale||!status||status.phase!=='learned'||!source||!learningSourcesEqual(source,{resourceId:current.id,generation:current.generation,capabilityRevision:current.capabilityRevision})||current.resourceType!=='host-controller'||current.availability!=='online')throw new Error('Inspect the current physical resource and restart learning before choosing a candidate.');
    const candidate=status.candidates.find(port=>port.descriptor.id===observedPortId&&port.contractHash===contractHash);
    if(!candidate||current.connections.length!==1||!current.connections.some(connection=>connection.route.transport==='hid'&&connection.route.baselineReady&&candidate.routeIds.includes(connection.route.id)))throw new Error('Choose one exact current native learned port and contract.');
    return candidate;
  }
  dispose():void{this.clear();this.disposed=true;this.listeners.clear();}
}
