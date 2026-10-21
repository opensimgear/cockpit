export type GameProvider = 'dcs-bios' | 'assetto-corsa-rally' | 'assetto-corsa' | 'assetto-corsa-competizione' | 'assetto-corsa-evo' | 'x-plane';
export type BuiltinGameId = 'dcs' | 'assetto-corsa-rally' | 'assetto-corsa' | 'assetto-corsa-competizione' | 'assetto-corsa-evo' | 'x-plane-11' | 'x-plane-12';
export type GameId = BuiltinGameId | `custom-${string}`;
export type GameBackground = { image: string; positionX: number; positionY: number };
export type GameLibraryCandidate = { id: string; client: string; name: string; installPath: string; executableCandidates: string[]; gameId?: BuiltinGameId; kind: 'game' | 'unknown' | 'tool'; appId?: string };
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
  activeAircraftModule?: string | null; aircraftModuleAutomatic?: boolean;
};
export type GameIntegrationContext = {
  status: 'waiting-profile' | 'waiting-game' | 'setup-required' | 'ready' | 'unsupported-platform' | 'error';
  message: string; processVerified: boolean; contextSession: string | null;
  providerSetupRevision: string | null; catalogueRevision: string | null;
};
export type GameVehicle = { id: string; name: string };
export type GameVehicleSupport = { supported: boolean; kind: 'aircraft' | 'car'; vehicles: GameVehicle[]; message?: string };
export type Game = { id: GameId; name: string; provider?: GameProvider | null; custom?: boolean; background?: GameBackground | null; defaultBackground?: GameBackground | null; installed: boolean; executable?: string | null; manualPath?: string | null; installPath?: string | null; source?: string; installations?: { executable: string; installPath: string; source: string }[]; launchSupported: boolean; processMonitoringSupported?: boolean; launchReason?: string | null; launchUnavailableReason?: string | null; integrationSetup?: GameIntegrationSetup; integrationContext?: GameIntegrationContext; vehicleSupport?: GameVehicleSupport; currentVehicle?: GameVehicle | null; vehicleStatus?: { status: 'ready' | 'waiting' | 'unavailable'; message: string } };
export type GameCleanup = { phase: 'idle' | 'restoring' | 'recovery-needed' | 'failed'; gameId?: GameId | null; retiredProfileId?: string | null; error?: string | null; steps?: string[]; recoveryArchive?: string | null };
export type DetectedGame = { gameId: GameId; name: string };
export type GamesState = { detectedGames?: DetectedGame[]; detectionErrors?: string[]; catalogue: Game[]; libraryCandidates?: GameLibraryCandidate[]; libraryDiagnostics?: string[]; addedGameId?: GameId; manualInstallations?: Record<string, string>; cleanup?: GameCleanup; error?: string | null };
export type GameCommandName = 'game_custom_save' | 'game_custom_remove' | 'game_background_save' | 'game_library_scan' | 'game_library_import' | 'game_get_state' | 'game_scan' | 'game_installation' | 'game_recover' | 'game_abandon' | 'game_integration_check' | 'game_integration_setup' | 'game_integration_cancel' | 'game_integration_recover' | 'game_integration_remove' | 'game_integration_settings_save';
export type GameCommand = GameCommandName;
export type GameRequest = { command: GameCommandName; gameId?: GameId; name?: string; path?: string | null; positionX?: number; positionY?: number; candidateId?: string; executable?: string | null; preserveCurrentSetup?: boolean; userFolder?: string; connection?: Record<string, unknown>; expectedSetupRevision?: string; aircraftModule?: string };

export const supportedGames: Game[] = [
  { id: 'dcs', name: 'DCS World', provider: 'dcs-bios', installed: false, launchSupported: false },
  { id: 'assetto-corsa-rally', name: 'Assetto Corsa Rally', provider: 'assetto-corsa-rally', installed: false, launchSupported: false },
  { id: 'x-plane-11', name: 'X-Plane 11', provider: 'x-plane', installed: false, launchSupported: false },
  { id: 'x-plane-12', name: 'X-Plane 12', provider: 'x-plane', installed: false, launchSupported: false },
  { id: 'assetto-corsa', name: 'Assetto Corsa', provider: 'assetto-corsa', installed: false, launchSupported: false },
  { id: 'assetto-corsa-competizione', name: 'Assetto Corsa Competizione', provider: 'assetto-corsa-competizione', installed: false, launchSupported: false },
  { id: 'assetto-corsa-evo', name: 'Assetto Corsa EVO', provider: 'assetto-corsa-evo', installed: false, launchSupported: false },
];

export function isGameId(value: unknown): value is GameId {
  return isBuiltinGameId(value) || isCustomGameId(value);
}

export function isBuiltinGameId(value: unknown): value is BuiltinGameId {
  return typeof value === 'string' && supportedGames.some(game => game.id === value);
}
/** Persisted profiles and owners remain readable while the registry is loading or unavailable. */
export function isCustomGameId(value: unknown): value is `custom-${string}` {
  return typeof value === 'string' && /^custom-[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
}
let registeredGames: readonly Game[] = supportedGames;
const catalogueListeners = new Set<(games: readonly Game[]) => void>();
const knownNames = new Map(supportedGames.map(game => [game.id, game.name]));
/** All selectors consume one native catalogue; pure helpers do not depend on Svelte. */
export const gameCatalogue = {
  subscribe(listener: (games: readonly Game[]) => void): () => void {
    catalogueListeners.add(listener); listener(registeredGames);
    return () => { catalogueListeners.delete(listener); };
  },
};
export function updateGameCatalogue(games: readonly Game[]): void {
  registeredGames = [...games];
  for (const game of registeredGames) knownNames.set(game.id, game.name);
  for (const listener of catalogueListeners) listener(registeredGames);
}
export function getGameCatalogue(): readonly Game[] { return registeredGames; }
export function isRegisteredGameId(value: unknown): value is GameId { return isGameId(value) && registeredGames.some(game => game.id === value); }
export function gameName(id: string): string { return knownNames.get(id as GameId) ?? id; }
export function gameHasIntegration(game: Game | undefined): boolean { return !!game?.provider; }
export function customGameError(name: string, executable: string): string {
  const title = name.trim();
  if (!title || [...title].length > 80 || /[\u0000-\u001f\u007f-\u009f]/.test(title)) return 'Enter a game name between 1 and 80 characters.';
  if (executable.trim() && !/^(?:[a-zA-Z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+|\/)/.test(executable.trim())) return 'Enter an absolute executable path.';
  if (/[\u0000-\u001f\u007f-\u009f]/.test(executable)) return 'The executable path must not contain control characters.';
  return '';
}
/** Installation controls keep custom registrations on their metadata API. */
export function gameInstallationRequest(game: Game, executable: string | null): GameRequest {
  return isCustomGameId(game.id)
    ? { command: 'game_custom_save', gameId: game.id, name: game.name, executable }
    : { command: 'game_installation', gameId: game.id, executable };
}

export function isSharedMemoryGame(gameId: unknown): boolean {
  return isBuiltinGameId(gameId) && gameId.startsWith('assetto-corsa');
}

export function gameExecutableName(gameId: GameId): string {
  return gameId.startsWith('x-plane-') ? 'X-Plane.exe' : gameId === 'dcs' ? 'DCS.exe'
    : gameId === 'assetto-corsa' ? 'acs.exe' : gameId === 'assetto-corsa-competizione' ? 'AC2-Win64-Shipping.exe'
    : gameId === 'assetto-corsa-evo' ? 'AssettoCorsaEVO.exe' : gameId === 'assetto-corsa-rally' ? 'acr.exe' : 'game executable';
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
  return catalogue.filter(game => showAll || game.installed || isCustomGameId(game.id));
}

export function gameIntegrationLabel(game: Game): string {
  if (!gameHasIntegration(game)) return 'Bindings and profile activation';
  if (!game.installed) return 'Game not installed';
  if (!game.integrationSetup) return 'Integration not checked';
  if (game.id === 'assetto-corsa-rally' && game.integrationSetup.state === 'ready' && game.integrationSetup.connection?.source === 'synthetic') return 'Synthetic test telemetry configured';
  if (isSharedMemoryGame(game.id) && ['ready', 'needs-attention'].includes(game.integrationSetup.state) && game.integrationSetup.connection?.source === 'shared-memory' && !gameIntegrationNativeVerified(game)) return 'Telemetry configured · verification needed';
  if (isSharedMemoryGame(game.id) && game.integrationSetup.state === 'ready' && !gameIntegrationNativeVerified(game)) return 'Native telemetry not verified';
  const labels: Record<GameIntegrationSetup['state'], string> = {
    'not-installed': 'Game not installed', 'setup-required': 'Integration setup required',
    'setting-up': 'Setting up integration', ready: 'Integration ready',
    'needs-attention': 'Integration needs attention', 'unsupported-platform': 'Setup unavailable on this platform',
  };
  return labels[game.integrationSetup.state];
}

export function gameIntegrationNativeVerified(game: Game): boolean {
  return isSharedMemoryGame(game.id) && game.integrationSetup?.verified === true && game.integrationSetup.connection?.source === 'shared-memory';
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
