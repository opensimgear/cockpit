/** Read-only samples from one exact native applied owner. No browser values enter the reducer. */
import {profileContractError,profileScalarAccepts,type AppliedProfileSnapshot,type DeepReadonly,type ProfileEndpoint,type ProfilePortContract} from './profiles';
import type {NativeProfileOwner} from './profile-runtime';
export const PROFILE_INSPECTION_SCHEMA_VERSION=1;
export type ProfileSignalValue={kind:'scalar';value:boolean|number|string}|{kind:'action-pack';value:{arguments:{id:string;value:boolean|number|string}[]}};
export type ProfileSignalSample={status:'known';value:ProfileSignalValue}|{status:'unknown'|'stale'|'unavailable'};
export type ProfileInspectedPort={endpoint:ProfileEndpoint;direction:'input'|'output';contract:ProfilePortContract;sample:ProfileSignalSample;observedUs:string|null};
export type ProfileInspectionPage={schemaVersion:1;owner:NativeProfileOwner;appliedSnapshot:AppliedProfileSnapshot;programHash:string;capturedUs:string;offset:number;total:number;ports:ProfileInspectedPort[]};
export type ProfileInspectionRequest={command:'profile_runtime_inspect';owner:NativeProfileOwner;nodeId?:string;offset:number;limit:number};
export type ProfileInspectionSend=(request:ProfileInspectionRequest)=>Promise<unknown>;
const object=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const exact=(value:unknown,keys:string[]):value is Record<string,unknown>=>object(value)&&Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
const uuid=(value:unknown):value is string=>typeof value==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const integer=(value:unknown,max=Number.MAX_SAFE_INTEGER):value is number=>typeof value==='number'&&Number.isSafeInteger(value)&&value>=0&&value<=max;
const hash=(value:unknown):value is string=>typeof value==='string'&&/^sha256:[a-f0-9]{64}$/.test(value);
const node=(value:unknown):value is string=>typeof value==='string'&&/^[A-Za-z][A-Za-z0-9._-]{0,79}$/.test(value);
const port=(value:unknown):value is string=>typeof value==='string'&&value.length>0&&new TextEncoder().encode(value).length<=80&&!/[\u0000-\u001f\u007f-\u009f]/.test(value);
const counter=(value:unknown):value is string=>typeof value==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(value)&&BigInt(value)<=18_446_744_073_709_551_615n;
const ownerFields=['session','generation','profileId','profileRevision','gameId'];
function owner(value:unknown):value is NativeProfileOwner{return exact(value,ownerFields)&&uuid(value.session)&&integer(value.generation)&&value.generation>0&&uuid(value.profileId)&&integer(value.profileRevision)&&value.profileRevision>0&&(value.gameId===null||['dcs','x-plane-11','x-plane-12','assetto-corsa-rally'].includes(String(value.gameId)));}
export function profileOwnersEqual(left:DeepReadonly<NativeProfileOwner>,right:DeepReadonly<NativeProfileOwner>):boolean{return owner(left)&&owner(right)&&ownerFields.every(key=>left[key as keyof NativeProfileOwner]===right[key as keyof NativeProfileOwner]);}
function frozen<T>(value:T):DeepReadonly<T>{const copy:T=JSON.parse(JSON.stringify(value));const freeze=(entry:unknown)=>{if(entry&&typeof entry==='object'){Object.values(entry).forEach(freeze);Object.freeze(entry);}};freeze(copy);return copy as DeepReadonly<T>;}
export function captureProfileOwner(value:DeepReadonly<NativeProfileOwner>):DeepReadonly<NativeProfileOwner>{if(!owner(value))throw new Error('Inspect an exact current native applied owner.');return frozen(value);}
function sample(value:unknown,contract:ProfilePortContract):value is ProfileSignalSample{
  if(!object(value))return false;
  if(value.status!=='known')return exact(value,['status'])&&['unknown','stale','unavailable'].includes(String(value.status));
  if(!exact(value,['status','value'])||!exact(value.value,['kind','value']))return false;
  if(contract.delivery==='state'||contract.delivery==='event')return value.value.kind==='scalar'&&profileScalarAccepts(contract.schema,value.value.value);
  const packed=value.value.value;
  return value.value.kind==='action-pack'&&exact(packed,['arguments'])&&Array.isArray(packed.arguments)&&packed.arguments.length===contract.parameters.length&&contract.parameters.every((parameter,index)=>{
    const argument=(packed.arguments as unknown[])[index];return exact(argument,['id','value'])&&argument.id===parameter.id&&profileScalarAccepts(parameter.schema,argument.value);
  })&&(contract.delivery!=='action'||!contract.display||typeof (packed.arguments[0] as {value:unknown})?.value==='string'&&[...(packed.arguments[0] as {value:string}).value].length<=contract.display.columns*contract.display.rows);
}
function inspectedPort(value:unknown,capturedUs:string,nodeId?:string):value is ProfileInspectedPort{
  if(!exact(value,['endpoint','direction','contract','sample','observedUs'])||!exact(value.endpoint,['nodeId','portId'])||!node(value.endpoint.nodeId)||!port(value.endpoint.portId)||(nodeId!==undefined&&value.endpoint.nodeId!==nodeId)||(value.direction!=='input'&&value.direction!=='output')||profileContractError(value.contract,value.direction)!=='')return false;
  if(!sample(value.sample,value.contract as ProfilePortContract)||!(value.observedUs===null||counter(value.observedUs)&&BigInt(value.observedUs)<=BigInt(capturedUs)))return false;
  return value.sample.status!=='known'||value.observedUs!==null;
}
/** A page is one timestamped native capture, not an acknowledgement that a sink executed. */
export class ProfileInspectorClient{
  constructor(private send:ProfileInspectionSend){}
  async inspect(run:DeepReadonly<NativeProfileOwner>,nodeId?:string,offset=0,limit=32):Promise<DeepReadonly<ProfileInspectionPage>>{
    const captured=captureProfileOwner(run);
    if(nodeId!==undefined&&!node(nodeId)||!integer(offset,1024)||!integer(limit,64)||limit===0)throw new Error('Choose a bounded applied-port inspection page.');
    const result=await this.send({command:'profile_runtime_inspect',owner:JSON.parse(JSON.stringify(captured)) as NativeProfileOwner,...(nodeId===undefined?{}:{nodeId}),offset,limit});
    if(!exact(result,['schemaVersion','owner','appliedSnapshot','programHash','capturedUs','offset','total','ports'])||result.schemaVersion!==PROFILE_INSPECTION_SCHEMA_VERSION||!owner(result.owner)||!profileOwnersEqual(captured,result.owner)||!exact(result.appliedSnapshot,['profileId','revision','semanticHash'])||result.appliedSnapshot.profileId!==captured.profileId||result.appliedSnapshot.revision!==captured.profileRevision||!hash(result.appliedSnapshot.semanticHash)||!hash(result.programHash)||!counter(result.capturedUs)||result.offset!==offset||!integer(result.total,1024)||!Array.isArray(result.ports)||result.ports.length>limit||offset+result.ports.length>result.total||offset<result.total&&result.ports.length===0||result.ports.some(value=>!inspectedPort(value,result.capturedUs as string,nodeId))||new Set(result.ports.map(value=>{const endpoint=(value as ProfileInspectedPort).endpoint;return JSON.stringify([endpoint.nodeId,endpoint.portId]);})).size!==result.ports.length)throw new Error('Native port samples do not match this applied owner and bounded page.');
    if(new TextEncoder().encode(JSON.stringify(result)).length>1_900_000)throw new Error('Native port inspection exceeds its reply budget.');
    return frozen(result as unknown as ProfileInspectionPage);
  }
}
