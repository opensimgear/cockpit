import {captureAutomaticStatus,type AutomaticGameId,type ProfileAutomaticClient,type ProfileAutomaticStatus} from './profile-automatic';
import type{DeepReadonly,ProfileSelectionMode}from'./profiles';
export type ProfileAutomaticState={status:DeepReadonly<ProfileAutomaticStatus>|null;pending:'mode'|'save-rule'|'remove-rule'|null;loading:boolean;stale:boolean;error:string};
type Client=Pick<ProfileAutomaticClient,'status'|'selectionMode'|'saveRule'|'removeRule'>;
/** Display/request fencing only. The native watcher owns every automatic decision and effect. */
export class ProfileAutomaticController{
  private value:ProfileAutomaticState={status:null,pending:null,loading:false,stale:true,error:''};private epoch=0;private reading=false;private disposed=false;
  private listeners=new Set<(state:DeepReadonly<ProfileAutomaticState>)=>void>();
  constructor(private client:Client){}
  get state():DeepReadonly<ProfileAutomaticState>{return Object.freeze(this.value);}
  subscribe(listener:(state:DeepReadonly<ProfileAutomaticState>)=>void):()=>void{this.listeners.add(listener);listener(this.state);return()=>this.listeners.delete(listener);}
  private set(next:Partial<ProfileAutomaticState>):void{if(this.disposed)return;this.value={...this.value,...next};for(const listener of this.listeners)listener(this.state);}
  async refresh():Promise<boolean>{
    if(this.disposed||this.reading||this.value.pending)return false;const epoch=this.epoch;this.reading=true;this.set({loading:true});
    try{const status=captureAutomaticStatus(await this.client.status());if(this.disposed||epoch!==this.epoch)return false;this.set({status,stale:false,error:''});return true;}
    catch(error){if(epoch===this.epoch)this.set({stale:true,error:error instanceof Error?error.message:String(error)});return false;}
    finally{this.reading=false;if(epoch===this.epoch)this.set({loading:false});}
  }
  selectionMode(mode:ProfileSelectionMode):Promise<boolean>{return this.run('mode',()=>this.client.selectionMode(mode));}
  saveRule(gameId:AutomaticGameId,profileId:string,priority:number,enabled:boolean):Promise<boolean>{return this.run('save-rule',()=>this.client.saveRule(gameId,profileId,priority,enabled));}
  removeRule(gameId:AutomaticGameId):Promise<boolean>{return this.run('remove-rule',()=>this.client.removeRule(gameId));}
  private async run(action:NonNullable<ProfileAutomaticState['pending']>,request:()=>Promise<DeepReadonly<ProfileAutomaticStatus>>):Promise<boolean>{
    if(this.disposed||this.value.pending)return false;if(!this.value.status||this.value.stale||this.value.status.readOnly){this.set({error:'Refresh current writable native automatic-profile status before changing selection or rules.'});return false;}
    const epoch=++this.epoch;this.set({pending:action,loading:false,stale:true,error:''});
    try{const status=captureAutomaticStatus(await request());if(this.disposed||epoch!==this.epoch)return false;this.set({status,stale:false});return true;}
    catch(error){if(epoch===this.epoch)this.set({error:error instanceof Error?error.message:String(error)});return false;}
    finally{if(epoch===this.epoch)this.set({pending:null});}
  }
  dispose():void{this.disposed=true;this.epoch++;this.listeners.clear();}
}
