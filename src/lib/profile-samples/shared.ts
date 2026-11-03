import {SharedObservations,type ObservationState} from '../streams/shared.ts';
import type {NativeProfileOwner} from '../profile-runtime.ts';
import type {DeepReadonly} from '../profiles.ts';
import {captureProfileNodeSamples,profileNodeSamplesTopic,type ProfileNodeSamplePort,type ProfileNodeSamples,type ProfileNodeSamplesState} from './models.ts';

export type ProfileNodeSamplesListener=(state:ProfileNodeSamplesState)=>void;
type Digest=(bytes:Uint8Array)=>Promise<ArrayBuffer>;
interface Entry{key:string;owner:NativeProfileOwner;nodeId:string;listeners:Set<ProfileNodeSamplesListener>;state:ProfileNodeSamplesState;stopTopic?:()=>void;opening:boolean;clockId?:string;needsBaseline:boolean;}
/** One exact owner/node topic and one immutable projection shared by its listeners. */
export class ProfileNodeSamplesObservers{
  private entries=new Map<string,Entry>();
  constructor(private observations=new SharedObservations(),private digest?:Digest){}
  observe(owner:DeepReadonly<NativeProfileOwner>,nodeId:string,listener:ProfileNodeSamplesListener):()=>void{
    const captured=JSON.parse(JSON.stringify(owner)) as NativeProfileOwner,key=JSON.stringify([captured.session,captured.generation,captured.profileId,captured.profileRevision,captured.gameId,nodeId]);
    let entry=this.entries.get(key);
    if(!entry){entry={key,owner:captured,nodeId,listeners:new Set(),state:Object.freeze({status:'connecting'}),opening:false,needsBaseline:true};this.entries.set(key,entry);}
    const current=entry;const registration:ProfileNodeSamplesListener=state=>listener(state);current.listeners.add(registration);
    try{registration(current.state);}catch{/* Isolate sample observers. */}
    if(this.active(current)&&!current.stopTopic&&!current.opening)this.open(current);
    let active=true;return()=>{if(!active)return;active=false;current.listeners.delete(registration);if(!current.listeners.size&&this.entries.get(key)===current){this.entries.delete(key);current.stopTopic?.();current.stopTopic=undefined;}};
  }
  private open(entry:Entry):void{
    entry.opening=true;
    void profileNodeSamplesTopic(entry.owner,entry.nodeId,this.digest).then(topic=>{
      if(!this.active(entry))return;
      try{
        const stop=this.observations.subscribe(topic,state=>this.accept(entry,topic,state));
        if(this.active(entry))entry.stopTopic=stop;else stop();
      }catch(error){if(this.active(entry))this.publish(entry,{status:'error',error:error instanceof Error?error.message:String(error)});}
      finally{entry.opening=false;}
    },error=>{entry.opening=false;if(this.active(entry))this.publish(entry,{status:'error',error:error instanceof Error?error.message:String(error)});});
  }
  private accept(entry:Entry,topic:string,observation:ObservationState):void{
    if(!this.active(entry))return;
    if(observation.status!=='ready'){entry.needsBaseline=true;this.publish(entry,{status:observation.status,error:observation.error});return;}
    const frame=observation.frame!;
    try{
      if(entry.needsBaseline&&frame.kind!=='snapshot')throw new Error('Waiting for a fresh native profile sample baseline.');
      if(entry.clockId&&frame.clockId!==entry.clockId&&frame.kind!=='snapshot')throw new Error('Native profile sample clock changed; waiting for a fresh baseline.');
      const value=captureProfileNodeSamples(frame,topic,entry.owner,entry.nodeId);
      if(frame.kind==='snapshot')entry.clockId=frame.clockId;
      entry.needsBaseline=false;this.publish(entry,{status:'ready',value});
    }catch(error){entry.needsBaseline=true;this.publish(entry,{status:'lost',error:error instanceof Error?error.message:String(error)});}
  }
  private active(entry:Entry):boolean{return this.entries.get(entry.key)===entry&&entry.listeners.size>0;}
  private publish(entry:Entry,state:ProfileNodeSamplesState):void{entry.state=Object.freeze(state);for(const listener of [...entry.listeners])if(this.active(entry)&&entry.listeners.has(listener)){try{listener(entry.state);}catch{/* Isolate consumers and preserve delivery acknowledgment. */}}}
}
let shared:ProfileNodeSamplesObservers|undefined;
export function observeProfileNodeSamples(owner:DeepReadonly<NativeProfileOwner>,nodeId:string,listener:ProfileNodeSamplesListener):()=>void{return(shared??=new ProfileNodeSamplesObservers()).observe(owner,nodeId,listener);}
export type {ProfileNodeSamplePort,ProfileNodeSamples,ProfileNodeSamplesState};
