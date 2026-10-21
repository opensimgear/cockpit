export type GameProvider = 'dcs-bios' | 'assetto-corsa-rally' | 'x-plane';
export type GameId = 'dcs' | 'assetto-corsa-rally' | 'x-plane-11' | 'x-plane-12';
export type GameIntegrationSetup = {
  state: 'not-installed' | 'setup-required' | 'setting-up' | 'ready' | 'needs-attention' | 'unsupported-platform';
  installed?: boolean; removable?: boolean; removalMessage?: string;
  message: string; userFolder?: string; candidates?: string[]; version?: string; cataloguePath?: string;
  connection?: Record<string, unknown>; progress?: string; recoveryNeeded?: boolean; repairable?: boolean;
  live?: 'waiting' | 'receiving' | 'stale'; liveMessage?: string; verified?: boolean;
  channels?: Record<string, { status: string; value?: unknown }>; optionalMaxRpm?: boolean;
  probeChannels?: Record<string, { status: 'available' | 'unknown' }>;
  aircraftExports?: { aircraft: string; detected: boolean; compatible: boolean; installed: boolean; path?: string | null; capability: string; message: string }[];
  providerSetupRevision?: string; catalogueRevision?: string | null; aircraftModule?: string | null;
};
export type GameIntegrationContext = {
  status: 'waiting-profile' | 'waiting-game' | 'setup-required' | 'ready' | 'unsupported-platform' | 'error';
  message: string; processVerified: boolean; contextSession: string | null;
  providerSetupRevision: string | null; catalogueRevision: string | null;
};
export type GameVehicle = { id: string; name: string };
export type GameVehicleSupport = { supported: boolean; kind: 'aircraft' | 'car'; vehicles: GameVehicle[]; message?: string };
export type Game = { id: GameId; name: string; provider: GameProvider; installed: boolean; executable?: string | null; installPath?: string | null; source?: string; installations?: { executable: string; installPath: string; source: string }[]; launchSupported: boolean; processMonitoringSupported?: boolean; launchReason?: string | null; launchUnavailableReason?: string | null; integrationSetup?: GameIntegrationSetup; integrationContext?: GameIntegrationContext; vehicleSupport?: GameVehicleSupport; currentVehicle?: GameVehicle | null; vehicleStatus?: { status: 'ready' | 'waiting' | 'unavailable'; message: string } };
export type GameCleanup = { phase: 'idle' | 'restoring' | 'recovery-needed' | 'failed'; gameId?: GameId | null; retiredProfileId?: string | null; error?: string | null; steps?: string[]; recoveryArchive?: string | null };
export type DetectedGame = { gameId: GameId; name: string };
export type GamesState = { detectedGames?: DetectedGame[]; detectionErrors?: string[]; catalogue: Game[]; manualInstallations?: Record<string, string>; cleanup?: GameCleanup; error?: string | null };
export type GameCommandName = 'game_get_state' | 'game_scan' | 'game_installation' | 'game_recover' | 'game_abandon' | 'game_integration_check' | 'game_integration_setup' | 'game_integration_cancel' | 'game_integration_recover' | 'game_integration_remove' | 'game_integration_settings_save';
export type GameCommand = GameCommandName;
export type GameRequest = { command: GameCommandName; gameId?: GameId; executable?: string | null; preserveCurrentSetup?: boolean; userFolder?: string; connection?: Record<string, unknown>; expectedSetupRevision?: string; aircraftModule?: string };

export const supportedGames: Game[] = [
  { id: 'dcs', name: 'DCS World', provider: 'dcs-bios', installed: false, launchSupported: false },
  { id: 'assetto-corsa-rally', name: 'Assetto Corsa Rally', provider: 'assetto-corsa-rally', installed: false, launchSupported: false },
  { id: 'x-plane-11', name: 'X-Plane 11', provider: 'x-plane', installed: false, launchSupported: false },
  { id: 'x-plane-12', name: 'X-Plane 12', provider: 'x-plane', installed: false, launchSupported: false },
];

export function gameExecutableName(gameId: GameId): string {
  return gameId.startsWith('x-plane-') ? 'X-Plane.exe' : gameId === 'dcs' ? 'DCS.exe' : 'acr.exe';
}

export function gameDisplayPath(path?: string | null): string {
  path = path ?? '';
  if (path.startsWith('\\\\?\\')) {
    if (path.slice(4, 8).toUpperCase() === 'UNC\\') path = '\\\\' + path.slice(8);
    else if (/^[a-z]:[\\/]/i.test(path.slice(4))) path = path.slice(4);
    else return path;
  }
  return /^[a-z]:[\\/]/i.test(path) || path.startsWith('\\\\') ? path.replaceAll('/', '\\') : path;
}

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
    'setting-up': 'Setting up integration', ready: 'Integration ready',
    'needs-attention': 'Integration needs attention', 'unsupported-platform': 'Setup unavailable on this platform',
  };
  return labels[game.integrationSetup.state];
}

export function gameIntegrationNativeVerified(game: Game): boolean {
  return game.id === 'assetto-corsa-rally' && game.integrationSetup?.verified === true && game.integrationSetup.connection?.source === 'shared-memory';
}

export function gameIntegrationReady(game: Game): boolean {
  return game.installed && game.integrationSetup?.state === 'ready' && (game.id === 'dcs' || game.provider === 'x-plane' || gameIntegrationNativeVerified(game));
}

export function gameIntegrationInstalled(game: Game): boolean {
  return game.integrationSetup?.installed ?? ['ready','needs-attention'].includes(game.integrationSetup?.state??'');
}

export function gameIntegrationVerificationLabel(game: Game): string {
  return gameIntegrationNativeVerified(game) ? 'Native telemetry verified' : 'Native telemetry not verified';
}

export function gameIntegrationLiveLabel(game: Game): string {
  if (game.id === 'assetto-corsa-rally' && game.integrationSetup?.connection?.source === 'synthetic') return game.integrationSetup.live === 'receiving' ? 'Receiving synthetic test data' : game.integrationSetup.live === 'stale' ? 'Synthetic test data is stale' : 'Waiting for synthetic test data';
  return game.integrationSetup?.live === 'receiving' ? 'Receiving game data'
    : game.integrationSetup?.live === 'stale' ? 'Game data is stale' : '';
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

export function gameIntegrationSetupRequest(game: Game, command: 'game_integration_check' | 'game_integration_setup' | 'game_integration_cancel' | 'game_integration_recover' | 'game_integration_remove', folder: string): GameRequest {
  return { command, gameId: game.id, ...(game.id === 'dcs' && folder.trim() ? { userFolder: folder.trim() } : {}) };
}

export function gameIntegrationSetupBlockedReason(game: Game, cleanup: GameCleanup | undefined, profileBlockedReason: string): string {
  if (!game.installed) return 'Install the game or select its executable first.';
  if (game.integrationSetup?.state === 'unsupported-platform') return game.integrationSetup.message;
  if (game.integrationSetup?.state === 'setting-up') return 'Integration setup is in progress.';
  if (game.integrationSetup?.recoveryNeeded) return 'Recover the interrupted setup before trying again.';
  if (game.id === 'dcs' && game.integrationSetup?.repairable === false) return game.integrationSetup.message;
  if (!cleanup) return 'Native cleanup metadata is unavailable. Refresh game status before changing provider setup.';
  if (!['idle', 'failed'].includes(cleanup.phase)) return 'Recover the previous owned setup before changing game integration.';
  if (profileBlockedReason) return profileBlockedReason;
  return '';
}

export function gameCleanupLabel(cleanup: GameCleanup): string {
  const labels: Record<GameCleanup['phase'], string> = { idle: 'Nothing to restore', restoring: 'Restoring previous owned setup', 'recovery-needed': 'Recovery needed', failed: 'Cleanup failed' };
  return labels[cleanup.phase];
}
