/** Explicit native runtime controls. Editor navigation and Save never call this client. */
import type { AppliedProfileSnapshot, DeepReadonly } from './profiles';
import type { CompanionOperation } from './companions';
import { isGameId, type GameId } from './games.ts';

export type ProfileGameId = GameId;
export type NativeProfileOwner = {
  session: string; generation: number; profileId: string; profileRevision: number; gameId: ProfileGameId | null;
};
export type NativeProfileStatus = {
  state: 'stopped' | 'running' | 'degraded' | 'faulted' | 'recovery-needed' | 'transitioning' | 'awaiting-game';
  appliedSnapshot: AppliedProfileSnapshot | null; owner: NativeProfileOwner | null;
  degradation: string[]; fault: string | null;
};
export type NativeProfileActivation = {
  activationId:string;profileId:string;
  phase:'preparing'|'activating'|'launching'|'deactivating'|'recovering'|'cancelling'|'succeeded'|'cancelled'|'failed';
  step?:string;steps?:{step:string;message:string}[];origin?:string;profileName?:string;vehicleName?:string|null;
  cancelRequested:boolean;canCancel:boolean;message:string;
  companions:{instanceId:string;publicationHash:string;name:string;operation?:CompanionOperation|null}[];
  outcome:null|'previous-active'|'active'|'inactive'|'recovery-needed';error:string|null;
};
export type ProfileRuntimeRequest = { command: 'profile_status' | 'profile_stop' | 'profile_recover' }
  | {command:'profile_activation_status'|'profile_activation_cancel';activationId:string}
  | { command: 'profile_apply'; id: string; expectedRevision: number; gameId?: ProfileGameId; expectedActiveOwner?:DeepReadonly<NativeProfileOwner>|null;activationId?:string };
export type ProfileRuntimeSend = (request: ProfileRuntimeRequest) => Promise<unknown>;

const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const revision = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
const text = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= 16384 && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/.test(value);
const game = isGameId;
const snapshot = (value: unknown): value is AppliedProfileSnapshot => object(value) && uuid(value.profileId) && revision(value.revision)
  && typeof value.semanticHash === 'string' && /^sha256:[a-f0-9]{64}$/.test(value.semanticHash);
const owner = (value: unknown): value is NativeProfileOwner => object(value) && uuid(value.session) && revision(value.generation)
  && uuid(value.profileId) && revision(value.profileRevision) && (value.gameId === null || game(value.gameId));

export function nativeProfileActivation(value:unknown,activationId:string):DeepReadonly<NativeProfileActivation>{
  if(!object(value)||value.activationId!==activationId||!uuid(value.activationId)||!uuid(value.profileId)
    ||typeof value.phase!=='string'||!['preparing','activating','launching','deactivating','recovering','cancelling','succeeded','cancelled','failed'].includes(value.phase)
    ||typeof value.cancelRequested!=='boolean'||typeof value.canCancel!=='boolean'||!text(value.message)
    ||!(value.vehicleName===undefined||value.vehicleName===null||text(value.vehicleName))
    ||!(value.outcome===null||typeof value.outcome==='string'&&['previous-active','active','inactive','recovery-needed'].includes(value.outcome))
    ||!(value.error===null||text(value.error))||!Array.isArray(value.companions)||value.companions.length>8
    ||!value.companions.every(item=>object(item)&&text(item.instanceId)&&item.instanceId.length<=128&&text(item.name)&&item.name.length<=1024
      &&typeof item.publicationHash==='string'&&/^sha256:[a-f0-9]{64}$/.test(item.publicationHash)
      &&(item.operation===undefined||item.operation===null||object(item.operation)&&item.operation.instanceId===item.instanceId&&item.operation.publicationHash===item.publicationHash))) {
    throw new Error('Profile activation progress is unavailable or belongs to another attempt.');
  }
  const result=JSON.parse(JSON.stringify(value)) as NativeProfileActivation;
  const freeze=(entry:unknown)=>{if(entry&&typeof entry==='object'){Object.values(entry).forEach(freeze);Object.freeze(entry);}};
  freeze(result);return result;
}

export function nativeProfileStatus(value: unknown): DeepReadonly<NativeProfileStatus> {
  if (!object(value) || typeof value.state !== 'string' || !['stopped', 'running', 'degraded', 'faulted', 'recovery-needed', 'transitioning', 'awaiting-game'].includes(value.state)
    || !(value.appliedSnapshot === null || snapshot(value.appliedSnapshot))
    || !(value.owner === null || owner(value.owner)) || !(value.fault === null || text(value.fault))
    || !Array.isArray(value.degradation) || value.degradation.length > 1024 || !value.degradation.every(text)) {
    throw new Error('Native profile runtime status is malformed. Refresh its current state.');
  }
  if (value.owner !== null && (!['running', 'degraded', 'awaiting-game'].includes(value.state)
    || !snapshot(value.appliedSnapshot) || value.owner.profileId !== value.appliedSnapshot.profileId
    || value.owner.profileRevision !== value.appliedSnapshot.revision)
    || ['running', 'degraded', 'awaiting-game'].includes(value.state) && value.owner === null) {
    throw new Error('Native profile runtime ownership is inconsistent. Refresh its current state.');
  }
  const captured = JSON.parse(JSON.stringify(value)) as NativeProfileStatus;
  const freeze = (entry: unknown) => { if (entry && typeof entry === 'object') { Object.values(entry).forEach(freeze); Object.freeze(entry); } };
  freeze(captured);
  return captured;
}

export class ProfileRuntimeClient {
  private readonly send: ProfileRuntimeSend;
  constructor(send: ProfileRuntimeSend) { this.send = send; }
  private async request(request: ProfileRuntimeRequest): Promise<DeepReadonly<NativeProfileStatus>> {
    return nativeProfileStatus(await this.send(request));
  }
  status(): Promise<DeepReadonly<NativeProfileStatus>> { return this.request({ command: 'profile_status' }); }
  apply(id: string, expectedRevision: number, gameId?: ProfileGameId, expectedActiveOwner?:DeepReadonly<NativeProfileOwner>|null,activationId?:string): Promise<DeepReadonly<NativeProfileStatus>> {
    if (!uuid(id) || !revision(expectedRevision) || gameId !== undefined && !game(gameId) || expectedActiveOwner!==undefined&&expectedActiveOwner!==null&&!owner(expectedActiveOwner)||activationId!==undefined&&!uuid(activationId)) {
      throw new Error('Apply requires an exact saved profile revision and a valid optional game ID.');
    }
    return this.request({ command: 'profile_apply', id, expectedRevision, ...(gameId === undefined ? {} : { gameId }), ...(expectedActiveOwner===undefined?{}:{expectedActiveOwner:expectedActiveOwner===null?null:JSON.parse(JSON.stringify(expectedActiveOwner))}),...(activationId?{activationId}:{}) });
  }
  activationStatus(activationId:string):Promise<DeepReadonly<NativeProfileActivation>>{return this.activation('profile_activation_status',activationId);}
  cancelActivation(activationId:string):Promise<DeepReadonly<NativeProfileActivation>>{return this.activation('profile_activation_cancel',activationId);}
  private async activation(command:'profile_activation_status'|'profile_activation_cancel',activationId:string):Promise<DeepReadonly<NativeProfileActivation>>{
    if(!uuid(activationId))throw new Error('Choose the exact profile activation attempt.');
    return nativeProfileActivation(await this.send({command,activationId}),activationId);
  }
  stop(): Promise<DeepReadonly<NativeProfileStatus>> { return this.request({ command: 'profile_stop' }); }
  recover(): Promise<DeepReadonly<NativeProfileStatus>> { return this.request({ command: 'profile_recover' }); }
}
