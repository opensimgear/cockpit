import { configurationError, type RuntimeConfig } from './runtime-config';
import { integrationError, type IntegrationConfig, type IntegrationProvider } from './integration-profile';

export type GameId = 'dcs' | 'assetto-corsa-rally';
export type Game = { id: GameId; name: string; provider: IntegrationProvider; installed: boolean; executable?: string | null; installPath?: string | null; source?: string; launchSupported: boolean; launchReason?: string | null; launchUnavailableReason?: string | null };
export type GameDevice = { gatewayId: string; moduleId: string; controller: 'keep' | 'enabled' | 'disabled'; config?: RuntimeConfig };
export type GameProfile = { schemaVersion: 1; id: string; gameId: GameId; name: string; devices: GameDevice[]; integration?: IntegrationConfig | null };
export type GameSession = { phase: 'idle' | 'validating' | 'applying' | 'launching' | 'waiting' | 'running' | 'restoring' | 'recovery-needed' | 'failed'; gameId?: GameId | null; profileId?: string | null; pid?: number | null; error?: string | null; steps?: string[]; bindingWarning?: boolean | string | null };
export type GamesState = { catalogue: Game[]; profiles: GameProfile[]; defaultProfiles: Record<string, string>; session: GameSession; error?: string | null };
export type GameCommandName = 'game_get_state' | 'game_scan' | 'game_installation' | 'game_profile_save' | 'game_profile_delete' | 'game_profile_default' | 'game_launch' | 'game_activate' | 'game_deactivate' | 'game_recover' | 'game_abandon';
export type GameCommand = GameCommandName;
export type GameRequest = { command: GameCommandName; gameId?: GameId; executable?: string | null; gameProfile?: GameProfile; profileId?: string; acceptBindingChanges?: boolean; preserveCurrentSetup?: boolean };

export const supportedGames: Game[] = [
  { id: 'dcs', name: 'DCS World', provider: 'dcs-bios', installed: false, launchSupported: false },
  { id: 'assetto-corsa-rally', name: 'Assetto Corsa Rally', provider: 'assetto-corsa-rally', installed: false, launchSupported: false },
];

export function visibleGames(catalogue: Game[], showAll: boolean): Game[] {
  return catalogue.filter(game => showAll || game.installed);
}

export function defaultGameProfile(state: Pick<GamesState, 'profiles' | 'defaultProfiles'>, gameId: GameId): GameProfile | undefined {
  const profiles = state.profiles.filter(profile => profile.gameId === gameId);
  return profiles.find(profile => profile.id === state.defaultProfiles[gameId]) ?? profiles[0];
}

export function newGameProfile(gameId: GameId, profiles: GameProfile[] = []): GameProfile {
  let number = 1;
  while (profiles.some(profile => profile.id === `${gameId}-${number}`)) number++;
  return { schemaVersion: 1, id: `${gameId}-${number}`, gameId, name: number === 1 ? 'Default' : `Profile ${number}`, devices: [], integration: null };
}

export function cloneGameProfile(profile: GameProfile): GameProfile {
  return JSON.parse(JSON.stringify(profile));
}

function sorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)).map(([key, item]) => [key, sorted(item)]));
  return value;
}

export function gameProfileEqual(a: GameProfile, b?: GameProfile | null): boolean {
  return !!b && JSON.stringify(sorted(a)) === JSON.stringify(sorted(b));
}

export function gameDeviceKey(device: Pick<GameDevice, 'gatewayId' | 'moduleId'>): string {
  return JSON.stringify([device.gatewayId, device.moduleId]);
}

export function profileChangesBindings(profile: GameProfile): boolean {
  return profile.devices.some(device => device.controller !== 'keep' || !!device.config);
}

const text = (value: unknown, limit: number): value is string => typeof value === 'string' && !!value.trim() && new TextEncoder().encode(value).length <= limit && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);

export function gameProfileError(value: unknown): string {
  if (!object(value) || value.schemaVersion !== 1 || !['dcs', 'assetto-corsa-rally'].includes(String(value.gameId))) return 'Choose a supported game and profile schema version 1.';
  if (typeof value.id !== 'string' || !/^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,63}$/.test(value.id)) return 'Choose a profile ID of 1–64 letters, numbers, dots, underscores or hyphens.';
  if (!text(value.name, 80)) return 'Choose a profile name of 1–80 UTF-8 bytes without control characters.';
  if (!Array.isArray(value.devices) || value.devices.length > 32) return 'A game profile supports up to 32 peripherals.';
  const assigned = new Set<string>();
  for (const device of value.devices) {
    if (!object(device) || !text(device.gatewayId, 127) || !text(device.moduleId, 127) || !['keep', 'enabled', 'disabled'].includes(String(device.controller))) return 'Choose a gateway, peripheral and controller mode for every assignment.';
    const key = gameDeviceKey({ gatewayId: device.gatewayId, moduleId: device.moduleId.toLowerCase() });
    if (assigned.has(key)) return 'A peripheral may appear only once on each gateway in a profile.';
    assigned.add(key);
    if (device.config !== undefined) {
      if (!object(device.config) || typeof device.config.name !== 'string' || !Number.isInteger(device.config.expanderMask) || Number(device.config.expanderMask) < 0 || Number(device.config.expanderMask) > 255 || !['buttons', 'axes', 'hats', 'encoders', 'selectors'].every(kind => Array.isArray((device.config as Record<string, unknown>)[kind]) && ((device.config as Record<string, unknown>)[kind] as unknown[]).every(Array.isArray))) return 'Capture a complete, valid peripheral configuration.';
      const error = configurationError(device.config as RuntimeConfig);
      if (error) return error;
    }
  }
  if (value.integration != null) {
    const integration = JSON.parse(JSON.stringify(value.integration));
    if (integration?.document?.provider === 'assetto-corsa-rally' && integration?.connection?.source === 'shared-memory' && integration.connection.processId == null) integration.connection.processId = 1;
    const error = integrationError(integration);
    if (error) return error;
    const provider = (value.integration as IntegrationConfig).document.provider;
    if (provider !== (value.gameId === 'dcs' ? 'dcs-bios' : 'assetto-corsa-rally')) return 'The integration provider must match the selected game.';
  }
  return '';
}

export function gameSessionLabel(session: GameSession): string {
  const labels: Record<GameSession['phase'], string> = { idle: 'Ready to play', validating: 'Checking your setup', applying: 'Activating profile', launching: 'Launching game', waiting: 'Waiting for game', running: 'Profile active', restoring: 'Restoring previous setup', 'recovery-needed': 'Recovery needed', failed: 'Launch failed' };
  return labels[session.phase];
}
