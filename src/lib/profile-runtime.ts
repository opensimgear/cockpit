/** Explicit native runtime controls. Editor navigation and Save never call this client. */
import type { AppliedProfileSnapshot, DeepReadonly } from './profiles';

export type ProfileGameId = 'dcs' | 'x-plane-11' | 'x-plane-12' | 'assetto-corsa-rally';
export type NativeProfileOwner = {
  session: string; generation: number; profileId: string; profileRevision: number; gameId: ProfileGameId | null;
};
export type NativeProfileStatus = {
  state: 'stopped' | 'running' | 'degraded' | 'faulted' | 'recovery-needed' | 'transitioning' | 'awaiting-game';
  appliedSnapshot: AppliedProfileSnapshot | null; owner: NativeProfileOwner | null;
  degradation: string[]; fault: string | null;
};
export type ProfileRuntimeRequest = { command: 'profile_status' | 'profile_stop' | 'profile_recover' }
  | { command: 'profile_apply'; id: string; expectedRevision: number; gameId?: ProfileGameId };
export type ProfileRuntimeSend = (request: ProfileRuntimeRequest) => Promise<unknown>;

const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const revision = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
const text = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= 16384 && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/.test(value);
const game = (value: unknown): value is ProfileGameId => typeof value === 'string' && ['dcs', 'x-plane-11', 'x-plane-12', 'assetto-corsa-rally'].includes(value);
const snapshot = (value: unknown): value is AppliedProfileSnapshot => object(value) && uuid(value.profileId) && revision(value.revision)
  && typeof value.semanticHash === 'string' && /^sha256:[a-f0-9]{64}$/.test(value.semanticHash);
const owner = (value: unknown): value is NativeProfileOwner => object(value) && uuid(value.session) && revision(value.generation)
  && uuid(value.profileId) && revision(value.profileRevision) && (value.gameId === null || game(value.gameId));

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
  apply(id: string, expectedRevision: number, gameId?: ProfileGameId): Promise<DeepReadonly<NativeProfileStatus>> {
    if (!uuid(id) || !revision(expectedRevision) || gameId !== undefined && !game(gameId)) {
      throw new Error('Apply requires an exact saved profile revision and a supported optional game context.');
    }
    return this.request({ command: 'profile_apply', id, expectedRevision, ...(gameId === undefined ? {} : { gameId }) });
  }
  stop(): Promise<DeepReadonly<NativeProfileStatus>> { return this.request({ command: 'profile_stop' }); }
  recover(): Promise<DeepReadonly<NativeProfileStatus>> { return this.request({ command: 'profile_recover' }); }
}
