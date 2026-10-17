/** Native HID candidate capture only. Learning never edits a graph, starts a run or accepts browser samples. */
import {resourcePortError,type ObservedResourcePort} from './profile-inventory';
import type {DeepReadonly} from './profiles';
export type ProfileLearningSource={resourceId:string;generation:string;capabilityRevision:string};
export type ProfileLearningStatus=ProfileLearningSource&{schemaVersion:1;learnId:string;phase:'listening'|'learned'|'stopped';reason:string;capturedUs:string;untilUs:string;candidates:ObservedResourcePort[]};
export type ProfileLearningCommand='profile_input_learn_start'|'profile_input_learn_status'|'profile_input_learn_cancel';
export type ProfileLearningRequest=({command:'profile_input_learn_start'}&ProfileLearningSource)|{command:'profile_input_learn_status'|'profile_input_learn_cancel';learnId:string};
export type ProfileLearningSend=(request:ProfileLearningRequest)=>Promise<unknown>;
const object=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const exact=(value:unknown,keys:string[]):value is Record<string,unknown>=>object(value)&&Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
const uuid=(value:unknown):value is string=>typeof value==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const text=(value:unknown,max:number,empty=false):value is string=>typeof value==='string'&&(empty||value.length>0)&&new TextEncoder().encode(value).length<=max&&!/[\u0000-\u001f\u007f-\u009f]/.test(value);
const hash=(value:unknown):value is string=>typeof value==='string'&&/^sha256:[a-f0-9]{64}$/.test(value);
const counter=(value:unknown):value is string=>typeof value==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(value)&&BigInt(value)<=18_446_744_073_709_551_615n;
export function learningSourceValid(value:unknown):value is ProfileLearningSource{return exact(value,['resourceId','generation','capabilityRevision'])&&text(value.resourceId,4096)&&counter(value.generation)&&hash(value.capabilityRevision);}
export function learningSourcesEqual(left:DeepReadonly<ProfileLearningSource>,right:DeepReadonly<ProfileLearningSource>):boolean{return left.resourceId===right.resourceId&&left.generation===right.generation&&left.capabilityRevision===right.capabilityRevision;}
function frozen<T>(value:T):DeepReadonly<T>{const copy:T=JSON.parse(JSON.stringify(value));const freeze=(entry:unknown)=>{if(entry&&typeof entry==='object'){Object.values(entry).forEach(freeze);Object.freeze(entry);}};freeze(copy);return copy as DeepReadonly<T>;}
function candidate(value:unknown):value is ObservedResourcePort{return exact(value,['descriptor','contractHash','routeIds','current'])&&resourcePortError(value.descriptor)===''&&object(value.descriptor)&&value.descriptor.direction==='output'&&object(value.descriptor.contract)&&['state','event'].includes(String(value.descriptor.contract.delivery))&&hash(value.contractHash)&&value.current===true&&Array.isArray(value.routeIds)&&value.routeIds.length===1&&typeof value.routeIds[0]==='string'&&uuid(value.routeIds[0].slice(4))&&value.routeIds[0].startsWith('hid:');}
export function captureLearningStatus(value:unknown):DeepReadonly<ProfileLearningStatus>{
  if(!exact(value,['schemaVersion','learnId','resourceId','generation','capabilityRevision','phase','reason','capturedUs','untilUs','candidates'])||value.schemaVersion!==1||!uuid(value.learnId)||!learningSourceValid({resourceId:value.resourceId,generation:value.generation,capabilityRevision:value.capabilityRevision})||!['listening','learned','stopped'].includes(String(value.phase))||!text(value.reason,16384,true)||!counter(value.capturedUs)||!counter(value.untilUs)||BigInt(value.untilUs)===0n||value.phase!=='stopped'&&BigInt(value.capturedUs)>BigInt(value.untilUs)||!Array.isArray(value.candidates)||value.candidates.length>32||!value.candidates.every(candidate)||new Set(value.candidates.map(port=>port.descriptor.id)).size!==value.candidates.length||(value.phase==='learned'?value.candidates.length===0:value.candidates.length!==0))throw new Error('Native input-learning capture is malformed or stale. Start a fresh capture explicitly.');
  return frozen(value as unknown as ProfileLearningStatus);
}
export class ProfileLearningClient{
  constructor(private send:ProfileLearningSend){}
  async start(source:DeepReadonly<ProfileLearningSource>):Promise<DeepReadonly<ProfileLearningStatus>>{
    if(!learningSourceValid(source))throw new Error('Choose the current native physical resource generation and capability revision.');
    const captured=frozen(source),status=captureLearningStatus(await this.send({command:'profile_input_learn_start',...captured}));
    if(!learningSourcesEqual(captured,status)||status.phase!=='listening')throw new Error('The native learner does not match the explicitly selected resource capture.');return status;
  }
  async status(learnId:string):Promise<DeepReadonly<ProfileLearningStatus>>{return this.request('profile_input_learn_status',learnId);}
  async cancel(learnId:string):Promise<DeepReadonly<ProfileLearningStatus>>{const status=await this.request('profile_input_learn_cancel',learnId);if(status.phase!=='stopped')throw new Error('Native input learning did not confirm cancellation.');return status;}
  private async request(command:'profile_input_learn_status'|'profile_input_learn_cancel',learnId:string):Promise<DeepReadonly<ProfileLearningStatus>>{if(!uuid(learnId))throw new Error('Choose a current native input-learning capture.');const status=captureLearningStatus(await this.send({command,learnId}));if(status.learnId!==learnId)throw new Error('The input-learning reply belongs to another capture.');return status;}
}
