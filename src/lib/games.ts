import { configurationError, type RuntimeConfig } from './runtime-config';
import { integrationError, type IntegrationConfig, type IntegrationProvider } from './integration-profile';

export type GameId = 'dcs' | 'assetto-corsa-rally';
export type GameIntegrationSetup = {
  state: 'not-installed' | 'setup-required' | 'setting-up' | 'ready' | 'needs-attention' | 'unsupported-platform';
  message: string; userFolder?: string; candidates?: string[]; version?: string; cataloguePath?: string;
  connection?: Record<string, unknown>; progress?: string; recoveryNeeded?: boolean; repairable?: boolean;
  live?: 'waiting' | 'receiving' | 'stale'; liveMessage?: string; verified?: boolean;
  channels?: Record<string, { status: string; value?: unknown }>; optionalMaxRpm?: boolean;
  probeChannels?: Record<string, { status: 'available' | 'unknown' }>;
};
export type Game = { id: GameId; name: string; provider: IntegrationProvider; installed: boolean; executable?: string | null; installPath?: string | null; source?: string; launchSupported: boolean; launchReason?: string | null; launchUnavailableReason?: string | null; integrationSetup?: GameIntegrationSetup };
export type GameDevice = { gatewayId: string; moduleId: string; controller: 'keep' | 'enabled' | 'disabled'; config?: RuntimeConfig };
export type GameProfile = { schemaVersion: 1; id: string; gameId: GameId; name: string; devices: GameDevice[]; integration?: IntegrationConfig | null };
export type GameSession = { phase: 'idle' | 'validating' | 'applying' | 'launching' | 'waiting' | 'running' | 'restoring' | 'recovery-needed' | 'failed'; gameId?: GameId | null; profileId?: string | null; pid?: number | null; error?: string | null; steps?: string[]; bindingWarning?: boolean | string | null };
export type GamesState = { catalogue: Game[]; profiles: GameProfile[]; defaultProfiles: Record<string, string>; session: GameSession; error?: string | null };
export type GameCommandName = 'game_get_state' | 'game_scan' | 'game_installation' | 'game_profile_save' | 'game_profile_delete' | 'game_profile_default' | 'game_launch' | 'game_activate' | 'game_deactivate' | 'game_recover' | 'game_abandon' | 'game_integration_check' | 'game_integration_setup' | 'game_integration_cancel' | 'game_integration_recover';
export type GameCommand = GameCommandName;
export type GameRequest = { command: GameCommandName; gameId?: GameId; executable?: string | null; gameProfile?: GameProfile; profileId?: string; acceptBindingChanges?: boolean; preserveCurrentSetup?: boolean; userFolder?: string; connection?: Record<string, unknown> };

export const supportedGames: Game[] = [
  { id: 'dcs', name: 'DCS World', provider: 'dcs-bios', installed: false, launchSupported: false },
  { id: 'assetto-corsa-rally', name: 'Assetto Corsa Rally', provider: 'assetto-corsa-rally', installed: false, launchSupported: false },
];

export function visibleGames(catalogue: Game[], showAll: boolean): Game[] {
  return catalogue.filter(game => showAll || game.installed);
}

export function gameIntegrationLabel(game: Game): string {
  if (!game.installed) return 'Game not installed';
  if (!game.integrationSetup) return 'Integration not checked';
  if (game.id === 'assetto-corsa-rally' && game.integrationSetup.state === 'ready' && game.integrationSetup.connection?.source === 'synthetic') return 'Synthetic test telemetry configured';
  if (game.id === 'assetto-corsa-rally' && ['ready', 'needs-attention'].includes(game.integrationSetup.state) && game.integrationSetup.connection?.source === 'shared-memory' && !gameIntegrationNativeVerified(game)) return 'Telemetry configured · verification needed';
  if (game.id === 'assetto-corsa-rally' && game.integrationSetup.state === 'ready' && !gameIntegrationNativeVerified(game)) return 'Native telemetry not verified';
  const labels: Record<GameIntegrationSetup['state'], string> = {
    'not-installed': 'Game not installed', 'setup-required': 'Integration setup required',
    'setting-up': 'Setting up integration', ready: 'Ready for integration',
    'needs-attention': 'Integration needs attention', 'unsupported-platform': 'Setup unavailable on this platform',
  };
  return labels[game.integrationSetup.state];
}

export function gameIntegrationNativeVerified(game: Game): boolean {
  return game.id === 'assetto-corsa-rally' && game.integrationSetup?.verified === true && game.integrationSetup.connection?.source === 'shared-memory';
}

export function gameIntegrationReady(game: Game): boolean {
  return game.installed && game.integrationSetup?.state === 'ready' && (game.id === 'dcs' || gameIntegrationNativeVerified(game));
}

export function gameIntegrationVerificationLabel(game: Game): string {
  return gameIntegrationNativeVerified(game) ? 'Native telemetry verified' : 'Native telemetry not verified';
}

export function gameIntegrationLiveLabel(game: Game): string {
  if (game.id === 'assetto-corsa-rally' && game.integrationSetup?.connection?.source === 'synthetic') return game.integrationSetup.live === 'receiving' ? 'Receiving synthetic test data' : game.integrationSetup.live === 'stale' ? 'Synthetic test data is stale' : 'Waiting for synthetic test data';
  return game.integrationSetup?.live === 'receiving' ? 'Receiving game data'
    : game.integrationSetup?.live === 'stale' ? 'Game data is stale' : 'Waiting for game data';
}

export function gameIntegrationUserFolder(game: Game): string {
  const setup = game.integrationSetup;
  return setup?.userFolder ?? (setup?.candidates?.length === 1 ? setup.candidates[0] : '');
}

export function gameIntegrationFolderError(folder: string): string {
  const path = folder.trim();
  if (!path) return 'Choose the DCS Saved Games folder.';
  if (/[\u0000-\u001f\u007f]/.test(path)) return 'The folder path must not contain control characters.';
  if (!/^(?:[a-zA-Z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+|\/)/.test(path)) return 'Enter an absolute folder path.';
  return '';
}

export function gameIntegrationTargetError(game: Game, folder: string): string {
  return game.id === 'dcs' ? gameIntegrationFolderError(folder) : '';
}

export function gameIntegrationSetupRequest(game: Game, command: 'game_integration_check' | 'game_integration_setup' | 'game_integration_cancel' | 'game_integration_recover', folder: string): GameRequest {
  return { command, gameId: game.id, ...(game.id === 'dcs' && folder.trim() ? { userFolder: folder.trim() } : {}) };
}

export function gameIntegrationSetupBlockedReason(game: Game, session: GameSession, integrationActive: boolean): string {
  if (!game.installed) return 'Install the game or select its executable first.';
  if (game.integrationSetup?.state === 'unsupported-platform') return game.integrationSetup.message;
  if (game.integrationSetup?.state === 'setting-up') return 'Integration setup is in progress.';
  if (game.integrationSetup?.recoveryNeeded) return 'Recover the interrupted setup before trying again.';
  if (game.id === 'dcs' && game.integrationSetup?.repairable === false) return game.integrationSetup.message;
  if (!['idle', 'failed'].includes(session.phase)) return 'Restore the active game profile before setting up integration.';
  if (integrationActive) return 'Stop the active integration before setting up integration.';
  return '';
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
    const error = integrationError(value.integration);
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
