import type {NativeProfileOwner} from './profile-runtime';
import type {DeepReadonly,ProfileControllerLayout,ProfilePort,ProfileResource} from './profiles';

export type DebugControllerLayout=ProfileControllerLayout;
export type DebugControllerChange={portId:string;value:boolean|number};
export type DebugControllerRequest=
  |{command:'profile_debug_controller_status';owner:DeepReadonly<NativeProfileOwner>;nodeId:string}
  |{command:'profile_debug_controller_input';owner:DeepReadonly<NativeProfileOwner>;nodeId:string;streamId:string;sequence:string;changes:DebugControllerChange[]};
export type DebugControllerSend=(request:DebugControllerRequest)=>Promise<unknown>;
export type DebugControllerSample=DebugControllerChange;
const uuid=(value:unknown):value is string=>typeof value==='string'&&/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
const identifier=(value:unknown):value is string=>typeof value==='string'&&/^[A-Za-z][A-Za-z0-9._-]{0,79}$/.test(value);
const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
const exact=(value:unknown,keys:string[])=>object(value)&&Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
const ownerValid=(value:unknown):value is NativeProfileOwner=>exact(value,['session','generation','profileId','profileRevision','gameId'])&&object(value)&&uuid(value.session)&&Number.isSafeInteger(value.generation)&&(value.generation as number)>0&&uuid(value.profileId)&&Number.isSafeInteger(value.profileRevision)&&(value.profileRevision as number)>0&&(value.gameId===null||typeof value.gameId==='string');
const sameOwner=(a:DeepReadonly<NativeProfileOwner>,b:DeepReadonly<NativeProfileOwner>)=>a.session===b.session&&a.generation===b.generation&&a.profileId===b.profileId&&a.profileRevision===b.profileRevision&&a.gameId===b.gameId;
const cloneOwner=(owner:DeepReadonly<NativeProfileOwner>):NativeProfileOwner=>({...owner});
const canonicalId=(id:unknown,prefix:string,max:number)=>typeof id==='string'&&new RegExp(`^${prefix}-[1-9]\\d*$`).test(id)&&Number(id.slice(prefix.length+1))<=max;
const labelValid=(value:unknown):value is string=>typeof value==='string'&&!!value.trim()&&value.trim()===value&&[...value].length<=80;
const axisChange=(change:DebugControllerChange)=>canonicalId(change.portId,'axis',16);
const sampleValid=(value:unknown):value is DebugControllerSample=>object(value)&&exact(value,['portId','value'])&&(
  canonicalId(value.portId,'button',128)&&typeof value.value==='boolean'||
  canonicalId(value.portId,'axis',16)&&Number.isSafeInteger(value.value)&&(value.value as number)>=-32768&&(value.value as number)<=32767||
  canonicalId(value.portId,'hat',4)&&Number.isSafeInteger(value.value)&&(value.value as number)>=-1&&(value.value as number)<=7);
// Object property order and the order of independent ports carry no contract meaning.
const structuralEqual=(a:unknown,b:unknown):boolean=>a===b||object(a)&&object(b)&&Object.keys(a).length===Object.keys(b).length&&Object.keys(a).every(key=>Object.hasOwn(b,key)&&structuralEqual(a[key],b[key]));

export function debugControllerResource(id:string,name='Debug controller'):ProfileResource{
  if(!identifier(id)||!labelValid(name))throw new Error('Use a valid debug controller identity and label.');
  const layout:DebugControllerLayout={buttons:Array.from({length:8},(_,i)=>`button-${i+1}`),axes:Array.from({length:2},(_,i)=>({portId:`axis-${i+1}`,range:'bipolar',fallback:0})),hats:[]};
  return{id,name,kind:'debug-controller',required:false,ports:debugControllerPorts(layout)};
}
export function debugControllerLayout(resource:{ports:readonly DeepReadonly<ProfilePort>[]}):DebugControllerLayout{
  const ports=(prefix:string,max:number)=>resource.ports.filter(port=>canonicalId(port.id,prefix,max)).sort((a,b)=>Number(a.id.slice(prefix.length+1))-Number(b.id.slice(prefix.length+1)));
  return{buttons:ports('button',128).map(port=>port.id),axes:ports('axis',16).map(port=>({portId:port.id,range:port.contract.delivery==='state'&&port.contract.schema.type==='integer'&&port.contract.schema.min===0?'unipolar':'bipolar',fallback:0})),hats:ports('hat',4).map(port=>port.id)};
}
export function debugControllerPorts(layout:DeepReadonly<DebugControllerLayout>):ProfileResource['ports']{
  const all=[...layout.buttons,...layout.axes.map(axis=>axis.portId),...layout.hats];
  if(all.length<1||all.length>128||layout.buttons.length>128||layout.axes.length>16||layout.hats.length>4||new Set(all).size!==all.length)throw new Error('Choose 1–128 total controls, at most 16 axes and four hats.');
  if(layout.buttons.some(id=>!canonicalId(id,'button',128))||layout.hats.some(id=>!canonicalId(id,'hat',4))||layout.axes.some(axis=>!canonicalId(axis.portId,'axis',16)||!['bipolar','unipolar'].includes(axis.range)||axis.fallback!==0))throw new Error('Use canonical control identifiers, ranges and neutral values.');
  return[
    ...layout.buttons.map(id=>({id,name:`Button ${id.slice(7)}`,direction:'output' as const,contract:{delivery:'state' as const,schema:{type:'boolean' as const},freshnessMs:1000,fallback:false}})),
    ...layout.axes.map(axis=>({id:axis.portId,name:`Axis ${axis.portId.slice(5)}`,direction:'output' as const,contract:{delivery:'state' as const,schema:{type:'integer' as const,min:axis.range==='bipolar'?-32768:0,max:32767,unit:null},freshnessMs:1000,fallback:0}})),
    ...layout.hats.map(id=>({id,name:`Hat ${id.slice(4)}`,direction:'output' as const,contract:{delivery:'state' as const,schema:{type:'integer' as const,min:-1,max:7,unit:null},freshnessMs:1000,fallback:-1}})),
  ];
}
export function debugControllerError(resource:unknown):string{
  if(!object(resource))return'Use a debug controller resource.';
  if(!identifier(resource.id)||resource.kind!=='debug-controller'||typeof resource.required!=='boolean'||!labelValid(resource.name)||!Array.isArray(resource.ports)||resource.ports.length<1||resource.ports.length>128)return'Use a valid debug controller label and 1–128 controls.';
  if(resource.ports.some(port=>!object(port)||!exact(port,['id','name','direction','contract'])||!labelValid(port.name)))return'Use canonical button, axis and hat port contracts.';
  const ports=resource.ports as ProfilePort[];
  if(new Set(ports.map(port=>port.id)).size!==ports.length)return'Use unique debug controller port IDs.';
  try{
    const canonical=debugControllerPorts(debugControllerLayout({ports}));
    if(canonical.length!==ports.length||canonical.some(expected=>{
      const port=ports.find(port=>port.id===expected.id);
      if(!port||port.direction!=='output'||port.contract?.delivery!=='state'||!Number.isSafeInteger(port.contract.freshnessMs)||port.contract.freshnessMs<1||port.contract.freshnessMs>600000)return true;
      return !structuralEqual({...expected.contract,freshnessMs:port.contract.freshnessMs},port.contract);
    }))return'Use canonical button, axis and hat port contracts.';
  }catch(error){return error instanceof Error?error.message:'Use a valid debug controller layout.';}
  return'';
}

type Stream={owner:NativeProfileOwner;possible:Set<string>;send:DebugControllerSend;nodeId:string;fail:(error:unknown)=>void};
type Batch={stream:Stream;changes:DebugControllerChange[];cleanup?:boolean};
/** One serial lane owns replay identity and keeps cleanup ahead of a reopened inspector's input. */
class DebugControllerTransport{
  readonly streamId=crypto.randomUUID();
  private sequence=0n;
  private queue:Batch[]=[];
  private sending=false;
  push(stream:Stream,changes:DebugControllerChange[]):boolean{
    const last=this.queue.at(-1);
    if(last&&last.stream===stream&&!last.cleanup&&last.changes.every(axisChange)&&changes.every(axisChange)){
      const latest=new Map(last.changes.map(change=>[change.portId,change]));for(const change of changes)latest.set(change.portId,change);last.changes=[...latest.values()];
    }else if(this.queue.length<128)this.queue.push({stream,changes});
    else return false;
    void this.pump();return true;
  }
  clear(stream:Stream):void{this.queue=this.queue.filter(batch=>batch.stream!==stream);this.release(stream);void this.pump();}
  private release(stream:Stream,first=false):void{
    if(!stream.possible.size)return;
    const batch:Batch={stream,cleanup:true,changes:[...stream.possible].map(portId=>({portId,value:false}))};
    if(first)this.queue.unshift(batch);else this.queue.push(batch);
  }
  private async pump():Promise<void>{
    if(this.sending)return;this.sending=true;
    try{
      while(this.queue.length){
        const batch=this.queue.shift()!;
        for(const change of batch.changes)if(change.value===true)batch.stream.possible.add(change.portId);
        try{
          await batch.stream.send({command:'profile_debug_controller_input',owner:cloneOwner(batch.stream.owner),nodeId:batch.stream.nodeId,streamId:this.streamId,sequence:(++this.sequence).toString(),changes:batch.changes});
          for(const change of batch.changes)if(change.value===false)batch.stream.possible.delete(change.portId);
        }catch(error){
          this.queue=this.queue.filter(next=>next.stream!==batch.stream);
          // An ambiguous failed press must release before another inspector can press again.
          if(!batch.cleanup){this.release(batch.stream,true);batch.stream.fail(error);}
        }
      }
    }finally{this.sending=false;}
  }
}
let editorTransport:DebugControllerTransport|null=null;
/** Reopened inspectors share bounded module-memory replay identity and ordered transport. */
export function createDebugControllerClient(send:DebugControllerSend,nodeId:string,onError:(error:unknown)=>void=()=>{}):DebugControllerClient{
  return new DebugControllerClient(send,nodeId,onError,editorTransport??=new DebugControllerTransport());
}
/** Exact-owner volatile stream: preserve discrete edges, coalesce only adjacent axis batches. */
export class DebugControllerClient{
  readonly streamId:string;
  private closed=false;
  private statusEpoch=0;
  private stream:Stream|null=null;
  private readonly send:DebugControllerSend;
  private readonly nodeId:string;
  private readonly onError:(error:unknown)=>void;
  private readonly transport:DebugControllerTransport;
  constructor(send:DebugControllerSend,nodeId:string,onError:(error:unknown)=>void=()=>{},transport=new DebugControllerTransport()){
    this.send=send;this.nodeId=nodeId;this.onError=onError;this.transport=transport;this.streamId=transport.streamId;
  }
  get currentOwner():DeepReadonly<NativeProfileOwner>|null{return this.stream?cloneOwner(this.stream.owner):null;}
  async status(owner:DeepReadonly<NativeProfileOwner>):Promise<DebugControllerSample[]>{
    if(this.closed||!ownerValid(owner))throw new Error('Debug controls require an exact active profile owner.');
    const epoch=++this.statusEpoch;
    const result=await this.send({command:'profile_debug_controller_status',owner:cloneOwner(owner),nodeId:this.nodeId});
    const debug=object(result)?result.debugController:null;
    if(!object(debug)||!ownerValid(debug.owner)||!sameOwner(owner,debug.owner)||debug.nodeId!==this.nodeId||!Array.isArray(debug.values))throw new Error('Debug controller status belongs to another owner or node.');
    if(('streamId' in debug&&debug.streamId!==null&&!uuid(debug.streamId))||('sequence' in debug&&debug.sequence!==null&&(typeof debug.sequence!=='string'||!/^(0|[1-9][0-9]*)$/.test(debug.sequence)||BigInt(debug.sequence)>18446744073709551615n))||('replayed' in debug&&typeof debug.replayed!=='boolean')||('pendingFrames' in debug&&(!Number.isSafeInteger(debug.pendingFrames)||(debug.pendingFrames as number)<0)))throw new Error('Debug controller status contains malformed stream metadata.');
    const values=debug.values;
    if(values.length>128||!values.every(sampleValid)||new Set(values.map(value=>value.portId)).size!==values.length)throw new Error('Debug controller status contains malformed control values.');
    if(this.closed||epoch!==this.statusEpoch)throw new Error('Debug controller status is no longer active.');
    if(this.stream&&!sameOwner(this.stream.owner,owner))this.clearOwner();
    if(!this.stream){
      const stream:Stream={owner:cloneOwner(owner),possible:new Set(),send:this.send,nodeId:this.nodeId,fail:error=>{
        if(this.stream!==stream)return;this.stream=null;++this.statusEpoch;if(!this.closed)this.onError(error);
      }};this.stream=stream;
    }
    for(const value of values)if(value.value===true)this.stream.possible.add(value.portId);
    return values.map(value=>({...value}));
  }
  push(owner:DeepReadonly<NativeProfileOwner>,changes:readonly DebugControllerChange[]):void{
    const stream=this.stream;
    if(this.closed||!stream||!ownerValid(owner)||!sameOwner(stream.owner,owner)||!changes.length)return;
    if(changes.length>128||!changes.every(sampleValid)||new Set(changes.map(change=>change.portId)).size!==changes.length){this.onError(new Error('Use unique canonical debug input changes.'));return;}
    if(!this.transport.push(stream,changes.map(change=>({...change})))){this.clearOwner();this.onError(new Error('Debug input queue is full; controls were released.'));}
  }
  /** Fence new input immediately, then release any possibly delivered presses after in-flight input. */
  clearOwner():void{++this.statusEpoch;const stream=this.stream;this.stream=null;if(stream)this.transport.clear(stream);}
  close():void{if(this.closed)return;this.closed=true;this.clearOwner();}
}
