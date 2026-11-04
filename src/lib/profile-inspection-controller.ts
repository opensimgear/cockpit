import {captureProfileOwner,profileOwnersEqual,type ProfileInspectionPage,type ProfileInspectorClient} from './profile-inspection';
import type {NativeProfileOwner} from './profile-runtime';
import type {DeepReadonly} from './profiles';
import type {ProfileNodeSamplesState} from './profile-samples/models';
export type ObserveNode=(owner:DeepReadonly<NativeProfileOwner>,nodeId:string,listener:(state:ProfileNodeSamplesState)=>void)=>()=>void;
export type ProfileInspectionState={owner:DeepReadonly<NativeProfileOwner>|null;nodeId:string|undefined;page:DeepReadonly<ProfileInspectionPage>|null;loading:boolean;stale:boolean;error:string;hasPrevious:boolean;hasNext:boolean};
/** Selection/page epochs keep an old run's samples out of a newer editor selection. */
export class ProfileInspectionController{
  private value:ProfileInspectionState={owner:null,nodeId:undefined,page:null,loading:false,stale:true,error:'',hasPrevious:false,hasNext:false};
  private epoch=0;private offsets:number[]=[];private disposed=false;
  private listeners=new Set<(state:DeepReadonly<ProfileInspectionState>)=>void>();
  private stops:(()=>void)[]=[];
  constructor(private client:Pick<ProfileInspectorClient,'inspect'>,private observe?:ObserveNode){}
  get state():DeepReadonly<ProfileInspectionState>{return Object.freeze(this.value);}
  subscribe(listener:(state:DeepReadonly<ProfileInspectionState>)=>void):()=>void{this.listeners.add(listener);listener(this.state);return()=>this.listeners.delete(listener);}
  private set(next:Partial<ProfileInspectionState>):void{if(this.disposed)return;this.value={...this.value,...next};for(const listener of this.listeners)listener(this.state);}
  private stopSamples():void{for(const stop of this.stops.splice(0))stop();}
  clear():void{this.epoch++;this.stopSamples();this.offsets=[];this.set({owner:null,nodeId:undefined,page:null,loading:false,stale:true,error:'',hasPrevious:false,hasNext:false});}
  async open(run:DeepReadonly<NativeProfileOwner>,nodeId?:string):Promise<boolean>{if(this.disposed)return false;const captured=captureProfileOwner(run);this.epoch++;this.stopSamples();this.offsets=[];this.set({owner:captured,nodeId,page:null,loading:false,stale:true,error:'',hasPrevious:false,hasNext:false});return this.load(0,'open');}
  refresh():Promise<boolean>{return this.load(this.value.page?.offset??0,'refresh');}
  next():Promise<boolean>{return this.value.hasNext?this.load(this.value.page!.offset+this.value.page!.ports.length,'next'):Promise.resolve(false);}
  previous():Promise<boolean>{return this.value.hasPrevious?this.load(this.offsets[this.offsets.length-2],'previous'):Promise.resolve(false);}
  private async load(offset:number,kind:'open'|'refresh'|'next'|'previous'):Promise<boolean>{
    if(this.disposed||this.value.loading||!this.value.owner)return false;
    const epoch=++this.epoch,run=this.value.owner,nodeId=this.value.nodeId,previous=this.value.page;
    this.stopSamples();this.set({loading:true,stale:true,error:''});
    try{
      // Leave room in the bounded node broker for other views and a page handoff.
      const page=await this.client.inspect(run,nodeId,offset,this.observe?16:32);if(this.disposed||epoch!==this.epoch)return false;
      if(!profileOwnersEqual(run,page.owner)||previous&&(page.programHash!==previous.programHash||page.appliedSnapshot.semanticHash!==previous.appliedSnapshot.semanticHash||page.total!==previous.total||BigInt(page.capturedUs)<BigInt(previous.capturedUs)))throw new Error('The applied inspection capture changed. Inspect the current native owner again.');
      if(kind==='open')this.offsets=[offset];else if(kind==='next')this.offsets.push(offset);else if(kind==='previous')this.offsets.pop();
      this.set({page,stale:false,hasPrevious:this.offsets.length>1,hasNext:page.offset+page.ports.length<page.total});
      this.watchPage(page,epoch);
      return true;
    }catch(error){if(epoch===this.epoch){this.stopSamples();this.set({stale:true,error:error instanceof Error?error.message:String(error)});}return false;}
    finally{if(epoch===this.epoch)this.set({loading:false});}
  }
  private watchPage(page:DeepReadonly<ProfileInspectionPage>,epoch:number):void{
    if(!this.observe||epoch!==this.epoch)return;
    const nodes=new Set(page.ports.map(port=>port.endpoint.nodeId));
    const pending=new Map([...nodes].map(node=>[node,'Waiting for native port samples.']));
    const seen=new Map<string,bigint>();
    for(const node of nodes){
      if(epoch!==this.epoch)break;
      const stop=this.observe(page.owner,node,state=>{
        if(epoch!==this.epoch||!this.value.page)return;
        if(state.status!=='ready'||!state.value){
          pending.set(node,state.error??'Waiting for native port samples.');
          this.set({stale:true,error:state.status==='connecting'?'':pending.get(node)});
          return;
        }
        const sample=state.value,current=this.value.page,at=BigInt(sample.capturedUs);
        const ports=new Map(sample.ports.map(port=>[port.endpoint.portId,port]));
        const matching=sample.nodeId===node&&profileOwnersEqual(page.owner,sample.owner)
          &&sample.programHash===page.programHash&&sample.appliedSnapshot.semanticHash===page.appliedSnapshot.semanticHash
          &&current.ports.filter(port=>port.endpoint.nodeId===node).every(port=>{
            const value=ports.get(port.endpoint.portId);
            return value?.direction===port.direction&&JSON.stringify(value.contract)===JSON.stringify(port.contract);
          });
        if(!matching||at<(seen.get(node)??0n)){
          const error='Native port samples changed scope or regressed. Waiting for a fresh applied capture.';
          pending.set(node,error);this.set({stale:true,error});return;
        }
        seen.set(node,at);pending.delete(node);
        const error=pending.values().next().value??'';
        if(at<BigInt(page.capturedUs)){this.set({stale:pending.size>0,error});return;}
        const updated=current.ports.map(port=>{
          if(port.endpoint.nodeId!==node)return port;
          const value=ports.get(port.endpoint.portId)!;
          return Object.freeze({...port,sample:value.sample,observedUs:value.observedUs});
        });
        this.set({page:Object.freeze({...current,ports:Object.freeze(updated),
          capturedUs:at>BigInt(current.capturedUs)?sample.capturedUs:current.capturedUs}),stale:pending.size>0,error});
      });
      if(epoch===this.epoch)this.stops.push(stop);else stop();
    }
  }
  dispose():void{this.clear();this.disposed=true;this.listeners.clear();}
}
