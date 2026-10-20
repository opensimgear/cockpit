import {nativeProfileStatus,type NativeProfileOwner,type NativeProfileStatus,type ProfileGameId,type ProfileRuntimeClient} from './profile-runtime';
import type{DeepReadonly}from'./profiles';

export type RuntimeEditorTarget={id:string;revision:number;baseRevision:number|null;dirty:boolean;readOnly:boolean};
export type ProfileActivationConfirmation={target:RuntimeEditorTarget;gameId?:ProfileGameId;previousOwner:DeepReadonly<NativeProfileOwner>};
export type RuntimeControlState={status:DeepReadonly<NativeProfileStatus>|null;pending:'apply'|'stop'|'recover'|null;activationConfirmation:DeepReadonly<ProfileActivationConfirmation>|null;loading:boolean;stale:boolean;error:string;statusError:string};
type Client=Pick<ProfileRuntimeClient,'status'|'apply'|'stop'|'recover'>;
/** UI admission and stale-reply fencing only; native owns every run and output. */
export class ProfileRuntimeController{
  private value:RuntimeControlState={status:null,pending:null,activationConfirmation:null,loading:false,stale:true,error:'',statusError:''};
  private listeners=new Set<(state:DeepReadonly<RuntimeControlState>)=>void>();
  private epoch=0;
  private reading=false;
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
    return this.run('apply',()=>this.client.apply(id,revision,gameId,previousOwner));
  }
  cancelActivation():void{this.set({activationConfirmation:null});}
  async confirmActivation():Promise<boolean>{
    const confirmation=this.value.activationConfirmation;
    if(!confirmation||this.value.pending)return false;
    this.set({activationConfirmation:null});
    if(this.value.stale||!this.value.status?.owner||(['session','generation','profileId','profileRevision','gameId'] as const).some(key=>this.value.status!.owner![key]!==confirmation.previousOwner[key])){this.set({error:'The active profile changed. Choose the profile to activate again.'});return false;}
    const {target,gameId,previousOwner}=confirmation;
    return this.run('apply',()=>this.client.apply(target.id,target.revision,gameId,previousOwner));
  }
  stop():Promise<boolean>{this.cancelActivation();return this.run('stop',()=>this.client.stop());}
  recover():Promise<boolean>{return this.run('recover',()=>this.client.recover());}
  dismissError():void{this.set({error:''});}
  private async run(action:NonNullable<RuntimeControlState['pending']>,request:()=>Promise<DeepReadonly<NativeProfileStatus>>):Promise<boolean>{
    if(this.disposed)return false;
    const interrupt=action==='stop'||action==='recover';
    if(this.value.pending&&(!interrupt||this.value.pending===action))return false;
    const epoch=++this.epoch;this.set({pending:action,activationConfirmation:null,loading:false,stale:true,error:''});let failed=false;
    try{const status=nativeProfileStatus(await request());if(this.disposed||epoch!==this.epoch)return false;this.set({status,stale:false,statusError:''});return true;}
    catch(error){if(epoch===this.epoch){failed=true;this.set({error:error instanceof Error?error.message:String(error),stale:true});}return false;}
    finally{if(epoch===this.epoch){this.set({pending:null});if(failed)await this.refresh();}}
  }
  dispose():void{this.disposed=true;this.epoch++;this.listeners.clear();}
}
