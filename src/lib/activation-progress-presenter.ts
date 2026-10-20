import type { DeepReadonly } from './profiles';
import type { NativeProfileActivation } from './profile-runtime';
import type { ActivationWindowProgress } from './profile-activation-window';

/** The current native operation decides the copy. A completed companion download
 * must never obscure a later visibility, mappings or game-launch operation. */
export function activationWindowProgress(progress:DeepReadonly<NativeProfileActivation>):ActivationWindowProgress {
  let message=progress.message;
  if(progress.phase!=='cancelling'&&progress.step?.startsWith('companion')) {
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
  return {operationId:progress.activationId,profileName:progress.profileName??progress.profileId,title:'Profile activation',phase:progress.phase==='launching'?'launching':progress.phase==='cancelling'?'cancelling':progress.phase==='preparing'?'preparing':'activating',message,details:[],canCancel:progress.canCancel&&!progress.cancelRequested};
}

type WindowAdapter={show:(progress:ActivationWindowProgress,cancel:()=>Promise<boolean>)=>Promise<boolean>;update:(progress:ActivationWindowProgress)=>Promise<boolean>;close:(id:string)=>Promise<void>;dispose:()=>void};
export class ActivationProgressPresenter {
  private current='';private disposed=false;
  constructor(private window:WindowAdapter,private cancel:(id:string)=>Promise<boolean>,private fallback:(progress:ActivationWindowProgress|null)=>void=()=>{}){}
  async sync(progress:DeepReadonly<NativeProfileActivation>|null):Promise<void>{
    if(this.disposed)return;
    if(!progress){const previous=this.current;this.current='';this.fallback(null);if(previous)await this.window.close(previous);return;}
    const payload=activationWindowProgress(progress);
    if(payload.operationId!==this.current){
      const previous=this.current;this.current=payload.operationId;
      if(previous)await this.window.close(previous);
      if(!this.disposed&&this.current===payload.operationId){const opened=await this.window.show(payload,()=>this.current===payload.operationId?this.cancel(payload.operationId):Promise.resolve(false));if(!this.disposed&&this.current===payload.operationId)this.fallback(opened?null:payload);}
    }else{const updated=await this.window.update(payload);if(!this.disposed&&this.current===payload.operationId)this.fallback(updated?null:payload);}
  }
  dispose(){this.disposed=true;this.current='';this.fallback(null);this.window.dispose();}
}
