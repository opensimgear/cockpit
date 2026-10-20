import {nativeProfileStatus,nativeProfileActivation,type NativeProfileActivation,type NativeProfileOwner,type NativeProfileStatus,type ProfileGameId,type ProfileRuntimeClient} from './profile-runtime';
import type{DeepReadonly}from'./profiles';

export type RuntimeEditorTarget={id:string;revision:number;baseRevision:number|null;dirty:boolean;readOnly:boolean};
export type ProfileActivationConfirmation={target:RuntimeEditorTarget;gameId?:ProfileGameId;previousOwner:DeepReadonly<NativeProfileOwner>};
export type RuntimeActivation={id:string;profileId:string;startedAt:number;cancelling:boolean};
export type RuntimeControlState={status:DeepReadonly<NativeProfileStatus>|null;pending:'apply'|'stop'|'recover'|null;activationConfirmation:DeepReadonly<ProfileActivationConfirmation>|null;activation:DeepReadonly<RuntimeActivation>|null;progress:DeepReadonly<NativeProfileActivation>|null;progressError:string;notice:string;loading:boolean;stale:boolean;error:string;statusError:string};
type Client=Pick<ProfileRuntimeClient,'status'|'apply'|'stop'|'recover'>&Partial<Pick<ProfileRuntimeClient,'activationStatus'|'cancelActivation'>>;
/** UI admission and stale-reply fencing only; native owns every run and output. */
export class ProfileRuntimeController{
  private value:RuntimeControlState={status:null,pending:null,activationConfirmation:null,activation:null,progress:null,progressError:'',notice:'',loading:false,stale:true,error:'',statusError:''};
  private listeners=new Set<(state:DeepReadonly<RuntimeControlState>)=>void>();
  private epoch=0;
  private reading=false;
  private readingProgress=false;
  private disposed=false;
  constructor(private client:Client){}
  get state():DeepReadonly<RuntimeControlState>{return Object.freeze(this.value);}
  subscribe(listener:(state:DeepReadonly<RuntimeControlState>)=>void):()=>void{this.listeners.add(listener);listener(this.state);return()=>this.listeners.delete(listener);}
  private set(value:Partial<RuntimeControlState>):void{if(this.disposed)return;this.value={...this.value,...value};for(const listener of this.listeners)listener(this.state);}
  async refresh():Promise<boolean>{
    if(this.disposed||this.reading||this.value.pending)return false;
    const epoch=this.epoch;this.reading=true;this.set({loading:true});
    try{const status=nativeProfileStatus(await this.client.status());if(this.disposed||epoch!==this.epoch)return false;this.set({status,stale:false,statusError:''});return true;}
    catch(error){if(epoch===this.epoch)this.set({stale:true,statusError:error instanceof Error?error.message:String(error)});return false;}
    finally{this.reading=false;if(epoch===this.epoch)this.set({loading:false});}
  }
  async apply(editor:RuntimeEditorTarget,gameId?:ProfileGameId):Promise<boolean>{
    if(this.disposed||this.value.pending)return false;
    if(editor.readOnly||editor.dirty||editor.baseRevision===null||editor.baseRevision!==editor.revision){this.set({error:'Save your changes before activating this profile.'});return false;}
    const previousOwner=this.value.status?.owner??null;
    if(previousOwner&&previousOwner.profileId!==editor.id){
      this.set({activationConfirmation:Object.freeze({target:Object.freeze({...editor}),...(gameId?{gameId}:{}),previousOwner}),error:''});return false;
    }
    const id=editor.id,revision=editor.baseRevision;
    return this.activate(id,revision,gameId,previousOwner);
  }
  cancelActivation():void{this.set({activationConfirmation:null});}
  async confirmActivation():Promise<boolean>{
    const confirmation=this.value.activationConfirmation;
    if(!confirmation||this.value.pending)return false;
    this.set({activationConfirmation:null});
    if(this.value.stale||!this.value.status?.owner||(['session','generation','profileId','profileRevision','gameId'] as const).some(key=>this.value.status!.owner![key]!==confirmation.previousOwner[key])){this.set({error:'The active profile changed. Choose the profile to activate again.'});return false;}
    const {target,gameId,previousOwner}=confirmation;
    return this.activate(target.id,target.revision,gameId,previousOwner);
  }
  private activate(id:string,revision:number,gameId:ProfileGameId|undefined,previousOwner:DeepReadonly<NativeProfileOwner>|null):Promise<boolean>{
    const activation=Object.freeze({id:crypto.randomUUID(),profileId:id,startedAt:Date.now(),cancelling:false});
    this.set({activation,progress:null,progressError:'',notice:''});
    return this.run('apply',()=>this.client.cancelActivation?this.client.apply(id,revision,gameId,previousOwner,activation.id):this.client.apply(id,revision,gameId,previousOwner));
  }
  async refreshProgress():Promise<void>{
    const activation=this.value.activation,epoch=this.epoch;
    if(this.disposed||this.readingProgress||this.value.pending!=='apply'||!activation||!this.client.activationStatus)return;
    this.readingProgress=true;
    try{
      const progress=nativeProfileActivation(await this.client.activationStatus(activation.id),activation.id);
      if(epoch===this.epoch&&this.value.activation?.id===activation.id)this.set({progress,progressError:''});
    }catch(error){if(epoch===this.epoch&&this.value.activation?.id===activation.id)this.set({progressError:error instanceof Error?error.message:String(error)});}
    finally{this.readingProgress=false;}
  }
  async cancelPendingActivation(operationId=this.value.activation?.id):Promise<boolean>{
    const activation=this.value.activation,epoch=this.epoch;
    if(this.disposed||this.value.pending!=='apply'||!activation||activation.id!==operationId||activation.cancelling||!this.client.cancelActivation)return false;
    this.set({activation:Object.freeze({...activation,cancelling:true}),error:''});
    try{
      const progress=nativeProfileActivation(await this.client.cancelActivation(activation.id),activation.id);
      if(epoch!==this.epoch||this.value.activation?.id!==activation.id)return false;
      this.set({progress,progressError:'',activation:Object.freeze({...activation,cancelling:progress.cancelRequested})});
      return progress.cancelRequested;
    }catch(error){if(epoch===this.epoch&&this.value.activation?.id===activation.id)this.set({activation,error:error instanceof Error?error.message:String(error)});return false;}
  }
  stop():Promise<boolean>{this.cancelActivation();return this.run('stop',()=>this.client.stop());}
  recover():Promise<boolean>{return this.run('recover',()=>this.client.recover());}
  dismissError():void{this.set({error:''});}
  private async run(action:NonNullable<RuntimeControlState['pending']>,request:()=>Promise<DeepReadonly<NativeProfileStatus>>):Promise<boolean>{
    if(this.disposed)return false;
    const interrupt=action==='stop'||action==='recover';
    if(this.value.pending&&(!interrupt||this.value.pending===action))return false;
    const activation=action==='apply'?this.value.activation:null;
    const epoch=++this.epoch;this.set({pending:action,activationConfirmation:null,...(action==='apply'?{}:{activation:null,progress:null,progressError:''}),notice:'',loading:false,stale:true,error:''});let failed=false;
    try{
      const status=nativeProfileStatus(await request());if(this.disposed||epoch!==this.epoch)return false;
      let notice='';
      if(activation&&(this.value.activation?.cancelling||this.value.progress?.cancelRequested)&&this.client.activationStatus){
        try{const progress=nativeProfileActivation(await this.client.activationStatus(activation.id),activation.id);if(epoch===this.epoch){this.set({progress});if(progress.cancelRequested&&progress.phase==='succeeded')notice=progress.message;}}catch{/* Activation succeeded even if its optional progress read is unavailable. */}
      }
      if(this.disposed||epoch!==this.epoch)return false;
      this.set({status,stale:false,statusError:'',error:'',progressError:'',notice});return true;
    }
    catch(error){if(epoch===this.epoch){
      failed=true;
      if(activation&&this.client.activationStatus){
        try{const progress=nativeProfileActivation(await this.client.activationStatus(activation.id),activation.id);if(epoch===this.epoch)this.set({progress,progressError:''});}catch{/* The original activation error remains authoritative when progress is unavailable. */}
      }
      if(epoch===this.epoch){
        const progress=this.value.progress;
        const cancelled=progress?.activationId===activation?.id&&progress?.phase==='cancelled';
        this.set({error:cancelled?'':error instanceof Error?error.message:String(error),notice:cancelled?progress.message:'',stale:true});
      }
    }return false;}
    finally{if(epoch===this.epoch){this.set({pending:null,activation:null});if(failed)await this.refresh();}}
  }
  dispose():void{this.disposed=true;this.epoch++;this.listeners.clear();}
}
