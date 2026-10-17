import {captureProfileOwner,profileOwnersEqual,type ProfileInspectionPage,type ProfileInspectorClient} from './profile-inspection';
import type {NativeProfileOwner} from './profile-runtime';
import type {DeepReadonly} from './profiles';
export type ProfileInspectionState={owner:DeepReadonly<NativeProfileOwner>|null;nodeId:string|undefined;page:DeepReadonly<ProfileInspectionPage>|null;loading:boolean;stale:boolean;error:string;hasPrevious:boolean;hasNext:boolean};
/** Selection/page epochs keep an old run's samples out of a newer editor selection. */
export class ProfileInspectionController{
  private value:ProfileInspectionState={owner:null,nodeId:undefined,page:null,loading:false,stale:true,error:'',hasPrevious:false,hasNext:false};
  private epoch=0;private offsets:number[]=[];private disposed=false;
  private listeners=new Set<(state:DeepReadonly<ProfileInspectionState>)=>void>();
  constructor(private client:Pick<ProfileInspectorClient,'inspect'>){}
  get state():DeepReadonly<ProfileInspectionState>{return Object.freeze(this.value);}
  subscribe(listener:(state:DeepReadonly<ProfileInspectionState>)=>void):()=>void{this.listeners.add(listener);listener(this.state);return()=>this.listeners.delete(listener);}
  private set(next:Partial<ProfileInspectionState>):void{if(this.disposed)return;this.value={...this.value,...next};for(const listener of this.listeners)listener(this.state);}
  clear():void{this.epoch++;this.offsets=[];this.set({owner:null,nodeId:undefined,page:null,loading:false,stale:true,error:'',hasPrevious:false,hasNext:false});}
  async open(run:DeepReadonly<NativeProfileOwner>,nodeId?:string):Promise<boolean>{if(this.disposed)return false;const captured=captureProfileOwner(run);this.epoch++;this.offsets=[];this.set({owner:captured,nodeId,page:null,loading:false,stale:true,error:'',hasPrevious:false,hasNext:false});return this.load(0,'open');}
  refresh():Promise<boolean>{return this.load(this.value.page?.offset??0,'refresh');}
  next():Promise<boolean>{return this.value.hasNext?this.load(this.value.page!.offset+this.value.page!.ports.length,'next'):Promise.resolve(false);}
  previous():Promise<boolean>{return this.value.hasPrevious?this.load(this.offsets[this.offsets.length-2],'previous'):Promise.resolve(false);}
  private async load(offset:number,kind:'open'|'refresh'|'next'|'previous'):Promise<boolean>{
    if(this.disposed||this.value.loading||!this.value.owner)return false;
    const epoch=++this.epoch,run=this.value.owner,nodeId=this.value.nodeId,previous=this.value.page;this.set({loading:true,stale:true,error:''});
    try{
      const page=await this.client.inspect(run,nodeId,offset,32);if(this.disposed||epoch!==this.epoch)return false;
      if(!profileOwnersEqual(run,page.owner)||previous&&(page.programHash!==previous.programHash||page.appliedSnapshot.semanticHash!==previous.appliedSnapshot.semanticHash||page.total!==previous.total||BigInt(page.capturedUs)<BigInt(previous.capturedUs)))throw new Error('The applied inspection capture changed. Inspect the current native owner again.');
      if(kind==='open')this.offsets=[offset];else if(kind==='next')this.offsets.push(offset);else if(kind==='previous')this.offsets.pop();
      this.set({page,stale:false,hasPrevious:this.offsets.length>1,hasNext:page.offset+page.ports.length<page.total});return true;
    }catch(error){if(epoch===this.epoch)this.set({stale:true,error:error instanceof Error?error.message:String(error)});return false;}
    finally{if(epoch===this.epoch)this.set({loading:false});}
  }
  dispose():void{this.clear();this.disposed=true;this.listeners.clear();}
}
