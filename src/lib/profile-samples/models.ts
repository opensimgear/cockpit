import {profileContractError,type AppliedProfileSnapshot,type DeepReadonly,type ProfileEndpoint,type ProfilePortContract} from '../profiles.ts';
import type {NativeProfileOwner} from '../profile-runtime.ts';
import {captureProfileOwner,profileNodeIdAccepts,profileOwnersEqual,profileSignalSampleAccepts,type ProfileSignalSample} from '../profile-inspection.ts';
import type {WireFrame} from '../streams/protocol.ts';

export type ProfileNodeSamplePort={endpoint:ProfileEndpoint;direction:'input'|'output';contract:ProfilePortContract;sample:ProfileSignalSample;observedUs:string|null};
export type ProfileNodeSamples={owner:NativeProfileOwner;nodeId:string;appliedSnapshot:AppliedProfileSnapshot;programHash:string;capturedUs:string;ports:ProfileNodeSamplePort[]};
export type ProfileNodeSamplesState=Readonly<{status:'connecting'|'ready'|'lost'|'error';value?:DeepReadonly<ProfileNodeSamples>;error?:string}>;
const record=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const exact=(value:unknown,keys:string[]):value is Record<string,unknown>=>record(value)&&Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
const hash=(value:unknown):value is string=>typeof value==='string'&&/^sha256:[a-f0-9]{64}$/.test(value);
const counter=(value:unknown):value is string=>typeof value==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(value)&&BigInt(value)<=18446744073709551615n;
function frozen<T>(value:T):DeepReadonly<T>{const copy=structuredClone(value);const freeze=(item:unknown)=>{if(item&&typeof item==='object'){Object.values(item).forEach(freeze);Object.freeze(item);}};freeze(copy);return copy as DeepReadonly<T>;}

/** Capture a value replacement with native per-port observation times. */
export function captureProfileNodeSamples(frame:WireFrame,topic:string,expectedOwner:DeepReadonly<NativeProfileOwner>,nodeId:string):DeepReadonly<ProfileNodeSamples>{
  if(frame.topic!==topic||frame.source!=='profile-samples'||!/^profile-samples:[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(frame.clockId)||frame.kind==='loss'||!counter(frame.capturedUs)||!record(frame.payload))throw new Error('Invalid native profile sample frame.');
  const value=frame.payload;
  if(new TextEncoder().encode(JSON.stringify(value)).length>1024*1024-2048||!exact(value,['owner','nodeId','appliedSnapshot','programHash','ports'])||!profileNodeIdAccepts(value.nodeId)||value.nodeId!==nodeId||!profileNodeIdAccepts(nodeId))throw new Error('Native profile samples do not match this node.');
  const owner=captureProfileOwner(value.owner as NativeProfileOwner);
  if(!profileOwnersEqual(expectedOwner,owner)||!exact(value.appliedSnapshot,['profileId','revision','semanticHash'])||value.appliedSnapshot.profileId!==owner.profileId||value.appliedSnapshot.revision!==owner.profileRevision||!hash(value.appliedSnapshot.semanticHash)||!hash(value.programHash)||!Array.isArray(value.ports)||value.ports.length>128)throw new Error('Native profile samples do not match this applied owner.');
  const identities=new Set<string>();
  for(const port of value.ports){
    if(!exact(port,['endpoint','direction','contract','sample','observedUs'])||!exact(port.endpoint,['nodeId','portId'])||port.endpoint.nodeId!==nodeId||typeof port.endpoint.portId!=='string'||!port.endpoint.portId.length||new TextEncoder().encode(port.endpoint.portId).length>80||/[\u0000-\u001f\u007f-\u009f]/.test(port.endpoint.portId)||!(port.direction==='input'||port.direction==='output')||profileContractError(port.contract,port.direction)!==''||!profileSignalSampleAccepts(port.sample,port.contract as ProfilePortContract)
      || !(counter(port.observedUs)||port.observedUs===null&&(port.sample as ProfileSignalSample).status!=='known'))throw new Error('Native profile sample contains an invalid port.');
    const identity=JSON.stringify([nodeId,port.endpoint.portId]);if(identities.has(identity))throw new Error('Native profile sample contains a duplicate port.');identities.add(identity);
  }
  return frozen({owner, nodeId, appliedSnapshot:value.appliedSnapshot as AppliedProfileSnapshot,programHash:value.programHash as string,capturedUs:frame.capturedUs,ports:value.ports as ProfileNodeSamplePort[]});
}

export async function profileNodeSamplesTopic(owner:DeepReadonly<NativeProfileOwner>,nodeId:string,digest:(bytes:Uint8Array<ArrayBuffer>)=>Promise<ArrayBuffer>=bytes=>crypto.subtle.digest('SHA-256',bytes)):Promise<string>{
  const captured=captureProfileOwner(owner);if(!profileNodeIdAccepts(nodeId))throw new Error('Choose a canonical profile node.');
  const bytes=new TextEncoder().encode(JSON.stringify([captured.session,captured.generation,captured.profileId,captured.profileRevision,captured.gameId,nodeId]));
  const result=new Uint8Array(await digest(bytes));return`profiles.samples.${Array.from(result,value=>value.toString(16).padStart(2,'0')).join('')}`;
}
