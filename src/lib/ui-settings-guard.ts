import type { GatewayRequest, GatewayState } from './gateway';
import type { GameId } from './games';
import type { NativeProfileOwner } from './profile-runtime';

export type LocalSettingsScope = 'desktop' | 'selection-mode' | 'game-rule' | 'game-preference' | 'game-provider';
export type LocalSettingsSnapshot = {
  scope: LocalSettingsScope; gameId: GameId | null; revision: string;
  settings: Record<string, unknown>; profileSession: { owner: NativeProfileOwner | null };
};
type Target = { key: string; scope: LocalSettingsScope; gameId?: GameId; read: GatewayRequest; write: boolean };
type Send = (request: GatewayRequest) => Promise<GatewayState>;
const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
const uuid = (value: unknown) => typeof value === 'string' && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(value);
const positive = (value: unknown) => typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
function capture(value: unknown, target: Target): LocalSettingsSnapshot {
  if (!object(value) || value.scope !== target.scope || value.gameId !== (target.gameId ?? null)
    || typeof value.revision !== 'string' || !/^sha256:[a-f0-9]{64}$/.test(value.revision)
    || !object(value.settings) || !object(value.profileSession)) throw new Error('Refresh current local settings before saving.');
  const owner = value.profileSession.owner;
  if (owner !== null && (!object(owner) || !uuid(owner.session) || !positive(owner.generation)
    || !uuid(owner.profileId) || !positive(owner.profileRevision) || !(owner.gameId === null || typeof owner.gameId === 'string')))
    throw new Error('Refresh current profile ownership before saving local settings.');
  return JSON.parse(JSON.stringify(value)) as LocalSettingsSnapshot;
}
function targetFor(request: GatewayRequest): Target | null {
  const command = request.command;
  if (command === 'profile_desktop_status' || command === 'profile_desktop_configure')
    return { key: 'desktop', scope: 'desktop', read: { command: 'profile_desktop_status' }, write: command === 'profile_desktop_configure' };
  if (['profile_automatic_status', 'profile_selection_mode', 'profile_game_rule_save', 'profile_game_rule_remove'].includes(command))
    return { key: 'automatic', scope: command.startsWith('profile_game_rule_') ? 'game-rule' : 'selection-mode',
      ...(command.startsWith('profile_game_rule_') ? { gameId: request.gameId } : {}),
      read: { command: 'profile_automatic_status' }, write: command !== 'profile_automatic_status' };
  if (['profile_game_candidates', 'profile_game_preference_save', 'profile_game_preference_remove'].includes(command))
    return { key: `preference:${request.gameId}`, scope: 'game-preference', gameId: request.gameId,
      read: { command: 'profile_game_candidates', gameId: request.gameId }, write: command !== 'profile_game_candidates' };
  return null;
}
function editedSettings(request: GatewayRequest): Record<string, unknown> {
  switch (request.command) {
    case 'profile_desktop_configure': return { enabled: request.enabled, scope: request.scope };
    case 'profile_selection_mode': return { mode: request.mode };
    case 'profile_game_rule_save': return { profileId: request.profileId, priority: request.priority, enabled: request.enabled };
    case 'profile_game_preference_save': return { profileId: request.profileId };
    default: return { remove: true };
  }
}

/** One instance per UI client/view. Save uses its displayed read's native
 * revision and owner, never a fresh snapshot fetched at the moment of writing.
 * Native admission remains authoritative; conflicts are surfaced without retry.
 */
export function guardUiSettings(send: Send): Send {
  const snapshots = new Map<string, LocalSettingsSnapshot>();
  const epochs = new Map<string, number>();
  return async request => {
    const target = targetFor(request);
    if (!target) return send(request);
    const epoch = (epochs.get(target.key) ?? 0) + 1;
    epochs.set(target.key, epoch);
    const selector = { scope: target.scope, ...(target.gameId ? { gameId: target.gameId } : {}) };
    if (!target.write) {
      // Capture before the displayed native read. A racing edit can therefore
      // cause a safe conflict, never authorize overwriting a newer revision.
      const base = capture((await send({ command: 'profile_local_settings_get', ...selector })).localSettings, target);
      const result = await send(request);
      if (epochs.get(target.key) === epoch) snapshots.set(target.key, base);
      return result;
    }
    const base = snapshots.get(target.key);
    if (!base) throw new Error('Refresh current local settings before saving.');
    try {
      const saved = capture((await send({ command: 'profile_local_settings_save', ...selector,
        expectedSettingsRevision: base.revision, expectedActiveOwner: base.profileSession.owner,
        settings: editedSettings(request) })).localSettings, target);
      const result = await send(target.read);
      if (epochs.get(target.key) === epoch) snapshots.set(target.key, saved);
      return result;
    } catch (error) {
      if (epochs.get(target.key) === epoch) snapshots.delete(target.key);
      throw error;
    }
  };
}
