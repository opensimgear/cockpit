import { captureRulesManifest, type RulesManifest } from './rule-pages.ts';
import { isGameId } from '../games.ts';
import { captureProfileSummaries, captureProfileSession, type DeepReadonly, type ProfileSession, type ProfileSummary } from '../profiles.ts';
import { nativeProfileStatus, type NativeProfileStatus } from '../profile-runtime.ts';
import { captureAutomaticStatus, type ProfileAutomaticStatus } from '../profile-automatic.ts';
import { uint64, type WireFrame } from '../streams/protocol.ts';

export type LibraryModel = { profiles: ProfileSummary[]; session: ProfileSession; diagnostics: string[]; readOnly: boolean; versions: Record<string, string> };
export type RuntimeModel = { session: ProfileSession; diagnostics: string[]; readOnly: boolean; profileSession: NativeProfileStatus; automatic: ProfileAutomaticStatus; preferredProfiles: Record<string,string> };
export type ModelState<T> = Readonly<{ status: 'connecting'|'ready'|'lost'|'error'; value?: DeepReadonly<T>; generation?: string; revision?: string; clockId?: string; session?: string; error?: string }>;
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
function valid(condition: unknown): asserts condition { if (!condition) throw new Error('Invalid profile observation. Waiting for a fresh baseline.'); }
function frozen<T>(value: T): DeepReadonly<T> {
  const copy = structuredClone(value);
  function freeze(item: unknown) { if (item && typeof item === 'object') { Object.values(item).forEach(freeze); Object.freeze(item); } }
  freeze(copy); return copy as DeepReadonly<T>;
}
function common(frame: WireFrame, topic: string): Record<string, unknown> {
  valid(frame.topic === topic && frame.source === 'profiles' && /^profiles:[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(frame.clockId) && frame.kind !== 'loss' && record(frame.payload));
  const value = frame.payload;
  valid(record(value.session) && uuid(value.session.selectedProfileId)
    && ['manual','automatic'].includes(value.session.selectionMode as string)
    && typeof value.readOnly === 'boolean' && Array.isArray(value.diagnostics) && value.diagnostics.length <= 1024 && value.diagnostics.every(item => typeof item === 'string'));
  valid(Object.keys(value.session).length === 5 && ['stopped','running','degraded','faulted','recovery-needed','transitioning','awaiting-game'].includes(value.session.state as string));
  nativeProfileStatus({state:'stopped',appliedSnapshot:value.session.appliedSnapshot,owner:null,degradation:[],fault:value.session.fault});
  return value;
}
export function captureLibrary(frame: WireFrame): DeepReadonly<LibraryModel> {
  const value = common(frame, 'profiles.library');
  const profiles = captureProfileSummaries(value.profiles);
  valid(record(value.versions) && Object.keys(value.versions).length === profiles.length && profiles.every(row => Object.hasOwn(value.versions as object,row.id) && uint64((value.versions as Record<string,unknown>)[row.id])));
  valid(Object.keys(value).length === 5);
  return frozen({profiles,session:captureProfileSession(value.session as ProfileSession),diagnostics:value.diagnostics,readOnly:value.readOnly,versions:value.versions}) as DeepReadonly<LibraryModel>;
}
export function captureRuntime(frame: WireFrame): DeepReadonly<RuntimeModel> {
  const value = common(frame, 'profiles.runtime');
  valid(Object.keys(value).length === 6 && record(value.preferredProfiles) && Object.keys(value.preferredProfiles).length <= 512
    && Object.entries(value.preferredProfiles).every(([game,id]) => isGameId(game) && uuid(id)));
  return frozen({session:captureProfileSession(value.session as ProfileSession),diagnostics:value.diagnostics,readOnly:value.readOnly,
    profileSession:nativeProfileStatus(value.profileSession),automatic:captureAutomaticStatus(value.automatic),preferredProfiles:value.preferredProfiles}) as DeepReadonly<RuntimeModel>;
}

/** Validate every small field before an issued immutable manifest can request pages. */
export function runtimeRulesManifest(frame: WireFrame): RulesManifest | undefined {
  if (!record(frame.payload) || !record(frame.payload.automatic) || Array.isArray(frame.payload.automatic.rules)) return undefined;
  const manifest = captureRulesManifest(frame.payload.automatic.rules);
  captureRuntime(withRuntimeRules(frame,[]));
  return manifest;
}
export function withRuntimeRules(frame: WireFrame, rules: readonly unknown[]): WireFrame {
  valid(record(frame.payload) && record(frame.payload.automatic));
  return {...frame,payload:{...frame.payload,automatic:{...frame.payload.automatic,rules}}};
}
