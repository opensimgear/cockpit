import type { DeepReadonly } from './profiles';
import type { NativeProfileActivation } from './profile-runtime';
import { profileProgressCanCancel, type ActivationWindowProgress } from './profile-activation-window';

function boundedLabel(value:string):string {
  const encoder=new TextEncoder();let bytes=0,result='';
  for(const character of value){const size=encoder.encode(character).length;if(bytes+size>118)break;bytes+=size;result+=character;}
  return result;
}

/** The current native operation decides the copy. A completed companion download
 * must never obscure a later visibility, mappings or game-launch operation. */
export function activationWindowProgress(progress:DeepReadonly<NativeProfileActivation>):ActivationWindowProgress {
  let message=progress.message;
  if(['preparing','activating'].includes(progress.phase)&&progress.step?.startsWith('companion')) {
    const current=progress.companions.filter(target=>target.operation&&!['ready','failed'].includes(target.operation.phase));
    const messages=current.flatMap(target=>{
      if(!target.operation)return [];
      const operation=target.operation;
      const label:Record<string,string>={opening:'Connecting', 'checking-cache':'Checking cached assets',downloading:'Downloading assets',rendering:'Drawing panel','awaiting-neutral':'Waiting for touch controls to be released'};
      const bytes=operation.phase==='downloading'&&operation.totalBytes?` · ${Math.floor((operation.completedBytes??0)/operation.totalBytes*100)}% (${Math.ceil((operation.completedBytes??0)/1024)} / ${Math.ceil(operation.totalBytes/1024)} KiB)`:'';
      return [`${target.name}: ${label[operation.phase]??operation.phase}${bytes}`];
    });
    if(messages.length)message=messages.join('; ');
  }
  const phase=progress.origin==='vehicle-switch'?'switching':progress.phase==='deactivating'?'deactivating':progress.phase==='recovering'?'recovering':progress.phase==='launching'?'launching':progress.phase==='cancelling'?'cancelling':progress.phase==='preparing'?'preparing':'activating';
  const profileName=progress.profileName??progress.profileId;
  return {operationId:progress.activationId,profileName:progress.origin==='vehicle-switch'&&progress.vehicleName?`${boundedLabel(profileName)} · ${boundedLabel(progress.vehicleName)}`:profileName,title:phase==='switching'?'Switching vehicle settings':phase==='deactivating'?'Profile deactivation':phase==='recovering'?'Output recovery':'Profile activation',phase,message,details:[],canCancel:profileProgressCanCancel({phase,canCancel:progress.canCancel&&!progress.cancelRequested})};
}

type WindowAdapter={show:(progress:ActivationWindowProgress,cancel:()=>Promise<boolean>)=>Promise<boolean>;update:(progress:ActivationWindowProgress)=>Promise<boolean>;close:(id:string)=>Promise<void>;dispose:()=>void};
export class ActivationProgressPresenter {
  private current='';private desired='';private disposed=false;private currentCanCancel=false;
  private queue:Promise<void>=Promise.resolve();
  constructor(private window:WindowAdapter,private cancel:(id:string)=>Promise<boolean>,private fallback:(progress:ActivationWindowProgress|null)=>void=()=>{}){}
  sync(progress:DeepReadonly<NativeProfileActivation>|null):Promise<void>{
    this.desired=progress?.activationId??'';
    this.currentCanCancel=!!progress&&activationWindowProgress(progress).canCancel;
    const next=this.queue.then(()=>this.render(progress));
    this.queue=next.catch(()=>{});
    return next;
  }
  private async render(progress:DeepReadonly<NativeProfileActivation>|null):Promise<void>{
    if(this.disposed)return;
    if(!progress){const previous=this.current;this.current='';this.fallback(null);if(previous)await this.window.close(previous);return;}
    const payload=activationWindowProgress(progress);
    if(payload.operationId!==this.current){
      const previous=this.current;this.current=payload.operationId;
      if(previous)await this.window.close(previous);
      if(!this.disposed&&this.current===payload.operationId){const opened=await this.window.show(payload,()=>!this.disposed&&this.current===payload.operationId&&this.desired===payload.operationId&&this.currentCanCancel?this.cancel(payload.operationId):Promise.resolve(false));if(!this.disposed&&this.current===payload.operationId)this.fallback(opened?null:payload);}
    }else{const updated=await this.window.update(payload);if(!this.disposed&&this.current===payload.operationId)this.fallback(updated?null:payload);}
  }
  dispose(){this.disposed=true;this.current='';this.desired='';this.currentCanCancel=false;this.fallback(null);this.window.dispose();}
}

export type ProfileCleanupProgressEvent={type:'start'|'finish';activationId:string;phase:'deactivating'|'recovering';error?:string};
type CleanupIntent={activationId:string;phase:'deactivating'|'recovering';finished:boolean};

/** A cleanup request can finish between native polls. Open immediately from its
 * dispatch event, retain it before native admission, then show the actual result.
 * None of this work is awaited by the output cleanup request itself. */
export class ApplicationProfileProgress {
  private intent?:CleanupIntent;
  private confirmation?:ReturnType<typeof globalThis.setTimeout>;
  private disposed=false;
  constructor(
    private presenter:ActivationProgressPresenter,
    private readExact:(id:string)=>Promise<DeepReadonly<NativeProfileActivation>|null>,
    private schedule:typeof globalThis.setTimeout=globalThis.setTimeout.bind(globalThis),
    private unschedule:typeof globalThis.clearTimeout=globalThis.clearTimeout.bind(globalThis),
  ){}

  async sync(progress:DeepReadonly<NativeProfileActivation>|null):Promise<void>{
    if(this.disposed)return;
    if(this.intent){
      if(progress?.activationId!==this.intent.activationId){
        if(!this.intent.finished||!progress)return;
        this.clearConfirmation();this.intent=undefined;
      }else if(this.intent.finished)return;
    }
    await this.presenter.sync(progress);
  }

  async cleanup(event:ProfileCleanupProgressEvent):Promise<void>{
    if(this.disposed)return;
    if(event.type==='start'){
      this.clearConfirmation();
      this.intent={activationId:event.activationId,phase:event.phase,finished:false};
      await this.presenter.sync(this.cleanupProgress(this.intent));
      return;
    }
    const intent=this.intent;
    if(!intent||intent.activationId!==event.activationId)return;
    intent.finished=true;
    let result:DeepReadonly<NativeProfileActivation>|null=null;
    try{result=await this.readExact(event.activationId);}catch{/* The request result still supplies a truthful terminal fallback. */}
    if(this.disposed||this.intent!==intent)return;
    const terminal=result?.activationId===intent.activationId&&['succeeded','failed','cancelled'].includes(result.phase)?result:null;
    const message=terminal?.message??(event.error
      ?`${intent.phase==='deactivating'?'Profile deactivation':'Output recovery'} did not complete. Check Cockpit for recovery details.`
      :intent.phase==='deactivating'?'Profile deactivated.':'Outputs recovered.');
    await this.presenter.sync({...this.cleanupProgress(intent),message});
    if(this.disposed||this.intent!==intent)return;
    this.confirmation=this.schedule(()=>{
      this.confirmation=undefined;
      if(this.disposed||this.intent!==intent)return;
      this.intent=undefined;
      void this.presenter.sync(null);
    },600);
  }

  dispose(){this.disposed=true;this.intent=undefined;this.clearConfirmation();this.presenter.dispose();}
  private clearConfirmation(){if(this.confirmation!==undefined)this.unschedule(this.confirmation);this.confirmation=undefined;}
  private cleanupProgress(intent:CleanupIntent):NativeProfileActivation{
    return {activationId:intent.activationId,profileId:'00000000-0000-0000-0000-000000000000',phase:intent.phase,origin:intent.phase==='deactivating'?'deactivation':'recovery',step:intent.phase,message:intent.phase==='deactivating'?'Deactivating profile…':'Recovering outputs…',canCancel:false,cancelRequested:false,companions:[],outcome:null,error:null};
  }
}
