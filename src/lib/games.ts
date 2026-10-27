export type GameProvider = 'dcs-bios' | 'assetto-corsa-rally' | 'assetto-corsa' | 'assetto-corsa-competizione' | 'assetto-corsa-evo' | 'x-plane';
export type GameId = string;
export type GameCrop = {x:number;y:number;width:number;height:number};
export type GameBackground = { crop?: GameCrop; image: string; positionX: number; positionY: number };
export type GameArtworkOption = {id:string;label:string;image:string};
export type GameTags = Record<string, string>;
export const UNTAGGED_TAG_FILTER = '@untagged';
export function tagError(tags: GameTags): string {
  const names = Object.values(tags).map(name => name.trim().toLowerCase());
  return names.length > 128 || new Set(names).size !== names.length || Object.values(tags).some(name => !name.trim() || [...name.trim()].length > 48 || /[\u0000-\u001f\u007f-\u009f]/.test(name))
    ? 'Use up to 128 tags with distinct names between 1 and 48 characters.' : '';
}
export function filterGamesByTag(games: readonly Game[], tag: string): Game[] {
  return games.filter(game => !tag || (tag === UNTAGGED_TAG_FILTER ? !game.tagIds?.length : game.tagIds?.includes(tag)));
}
export type GameListFilterState = {name:string;tags:string[];libraries:string[];integrations:('available'|'unavailable')[];profiles:('yes'|'no')[]};
export function emptyGameListFilters():GameListFilterState { return {name:'',tags:[],libraries:[],integrations:[],profiles:[]}; }
function gameLibraries(game:Game):string[] {
  const installations=game.installations?.length?game.installations:[{source:game.source,sources:[]}];
  return [...new Set(installations.flatMap(installation=>{
    const sources=[...new Set([installation.source,...(installation.sources??[]).map(source=>source.client)].filter((source):source is string=>!!source).map(source=>['local','manual','standalone'].includes(source)?'disk':source))];
    const platforms=sources.filter(source=>source!=='disk');
    return platforms.length?platforms:['disk'];
  }))];
}
export function gameListLibraries(games:readonly Game[]):{value:string;label:string}[] {
  return [...new Set(games.flatMap(gameLibraries))].map(value=>({value,label:value==='disk'?'Local':gameLibraryClientName(value)})).sort((left,right)=>left.label.localeCompare(right.label));
}
export function filterGameList(games:readonly Game[], filters:GameListFilterState, profiles:readonly {isDefault:boolean;gameIds?:readonly string[]}[]=[]):Game[] {
  const name=filters.name.trim().toLowerCase();
  const profiledGames=new Set(profiles.filter(profile=>!profile.isDefault).flatMap(profile=>profile.gameIds??[]));
  return games.filter(game=>(!name||game.name.toLowerCase().includes(name))
    &&(!filters.tags.length||filters.tags.some(tag=>tag===UNTAGGED_TAG_FILTER?!game.tagIds?.length:game.tagIds?.includes(tag)))
    &&(!filters.libraries.length||gameLibraries(game).some(library=>filters.libraries.includes(library)))
    &&(!filters.integrations.length||filters.integrations.includes(gameHasIntegration(game)?'available':'unavailable'))
    &&(!filters.profiles.length||filters.profiles.includes(profiledGames.has(game.id)?'yes':'no')));
}
export function gameLibraryClientName(client: string): string {
  return ({steam: 'Steam', epic: 'Epic Games', battlenet: 'Battle.net', ea: 'EA app', ubisoft: 'Ubisoft Connect', gog: 'GOG', xbox: 'Xbox'} as Record<string, string>)[client] ?? client;
}
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
export type Game = { id: GameId; name: string; tagIds?: string[]; provider?: GameProvider | null; background?: GameBackground | null; defaultBackground?: GameBackground | null; installed: boolean; executable?: string | null; manualPath?: string | null; installPath?: string | null; source?: string; integrationKey?: string; selectedInstallationId?: string; installations?: GameInstallation[]; launchSupported: boolean; processMonitoringSupported?: boolean; launchReason?: string | null; launchUnavailableReason?: string | null; integrationSetup?: GameIntegrationSetup; integrationContext?: GameIntegrationContext; vehicleSupport?: GameVehicleSupport; currentVehicle?: GameVehicle | null; vehicleStatus?: { status: 'ready' | 'waiting' | 'unavailable'; message: string } };
export type DetectedGame = { gameId: GameId; name: string };
export type GameLibrarySource = { client:string;appId:string;installPath:string;launcher?:string };
export type GameInstallation = { id: string; executable?: string; installPath: string; source: string; sources?: GameLibrarySource[]; available: boolean; executableCandidates: string[] };
export type GameDiscoveryEntry = {
  image?: string | null; tagIds?: string[]; tagSource?: string; id: string; action: 'add' | 'update' | 'remove'; selected: boolean; gameId?: string; installationId?: string; name: string; library: string; installPath: string; previousPath?: string; executableCandidates: string[]; executable?: string; updateKind?: 'add-installation' | 'replace-path'; integration?: string; message?: string };
export type GameDiscoveryObservation = {id:string;client:string;name:string;installPath:string;executableCandidates:string[];gameId?:string;kind?:string};
export type GameDiscovery = { scanId: string; status: 'idle' | 'scanning' | 'ready' | 'applied' | 'failed'; libraries: { id: string; name: string; status: 'queued' | 'scanning' | 'preparing' | 'complete' | 'unavailable' | 'failed'; found: number; message?: string }[]; entries: GameDiscoveryEntry[]; observations?:GameDiscoveryObservation[]; diagnostics: string[]; firstStart: boolean };
export type GameDiscoveryAction = { id: string; selected: boolean; executable?: string | null; replaceInstallationId?: string | null; gameId?: string };
export type GamesState = { artworkPending?: boolean; backgroundPreview?: {image:string}; tags?: GameTags; detectedGames?: DetectedGame[]; detectionErrors?: string[]; catalogue: Game[]; discovery?: GameDiscovery; addedGameId?: GameId; error?: string | null };
export type GameCommandName = 'game_metadata_get' | 'game_artwork_list' | 'game_artwork_preview' | 'game_background_preview' | 'game_tag_definitions_save' | 'game_tags_save' | 'game_add' | 'game_save' | 'game_rescan' | 'game_remove' | 'game_background_save' | 'game_discovery_start' | 'game_discovery_apply' | 'game_get_state' | 'game_installation_select' | 'game_integration_check' | 'game_integration_setup' | 'game_integration_cancel' | 'game_integration_recover' | 'game_integration_remove' | 'game_integration_settings_save';
export type GameCommand = GameCommandName;
export type GameRequest = { command: GameCommandName; url?:string; imageData?:string; imagePath?:string; installPath?:string; crop?:GameCrop; tags?: GameTags; expectedTags?: GameTags; expectedTagIds?: string[]; tagIds?: string[]; gameId?: GameId; installationId?: string; name?: string; path?: string | null; paths?: string[]; scanId?: string; actions?: GameDiscoveryAction[]; positionX?: number; positionY?: number; executable?: string | null; userFolder?: string; connection?: Record<string, unknown>; expectedSetupRevision?: string; aircraftModule?: string };

export function isGameId(value: unknown): value is GameId {
  return typeof value === 'string' && /^[a-z0-9][a-z0-9-]{0,95}$/.test(value);
}
let registeredGames: readonly Game[] = [];
const catalogueListeners = new Set<(games: readonly Game[]) => void>();
const knownNames = new Map<string, string>();
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
export function gameMetadataError(name: string, executable: string): string {
  const title = name.trim();
  if (!title || [...title].length > 80 || /[\u0000-\u001f\u007f-\u009f]/.test(title)) return 'Enter a game name between 1 and 80 characters.';
  if (executable.trim() && !/^(?:[a-zA-Z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+|\/)/.test(executable.trim())) return 'Enter an absolute executable path.';
  if (/[\u0000-\u001f\u007f-\u009f]/.test(executable)) return 'The executable path must not contain control characters.';
  return '';
}
export function gameInstallationRequest(game: Game, executable: string | null): GameRequest {
  return { command: 'game_save', gameId: game.id, ...(game.selectedInstallationId ? { installationId: game.selectedInstallationId } : {}), executable };
}
export function isSharedMemoryGame(gameId: unknown): boolean {
  const game = registeredGames.find(game => game.id === gameId);
  return !!game?.provider?.startsWith('assetto-corsa');
}
function sharedMemoryGame(game: Game): boolean { return !!game.provider?.startsWith('assetto-corsa'); }

export function gameDisplayPath(path?: string | null): string {
  path = path ?? '';
  if (path.startsWith('\\\\?\\')) {
    if (path.slice(4, 8).toUpperCase() === 'UNC\\') path = '\\\\' + path.slice(8);
    else if (/^[a-z]:[\\/]/i.test(path.slice(4))) path = path.slice(4);
    else return path;
  }
  return /^[a-z]:[\\/]/i.test(path) || path.startsWith('\\\\') ? path.replaceAll('/', '\\') : path;
}

export function gameExecutableDisplayPath(path?: string | null, installationPath?: string | null): string {
  const executable = gameDisplayPath(path), installation = gameDisplayPath(installationPath);
  if (!installation) return executable;
  const windows = /^[a-z]:[\\/]/i.test(installation) || installation.startsWith('\\\\');
  const root = installation.replace(windows ? /[\\/]+$/ : /\/+$/, '');
  const prefix = root + (windows ? '\\' : '/');
  const matches = windows ? executable.toLowerCase().startsWith(prefix.toLowerCase()) : executable.startsWith(prefix);
  return matches ? executable.slice(prefix.length) : executable;
}

export function gameAddError(name:string,root:string,executable:string):string {
  if (!root.trim() || gameMetadataError('Game',root.trim())) return 'Choose an absolute game folder.';
  const path=resolveGameExecutablePath(executable.trim(),root.trim());
  const invalid=gameMetadataError(name,path);if(invalid)return invalid;
  if (!executable.trim() || !/\.exe$/i.test(path)) return 'Choose a game executable (.exe).';
  return gameExecutableDisplayPath(path,root.trim())===gameDisplayPath(path)?'Choose an executable inside the selected game folder.':'';
}

export function gameAddRequest(name:string,root:string,executable:string,imagePath=''):GameRequest {
  const invalid=gameAddError(name,root,executable);if(invalid)throw new Error(invalid);
  return {command:'game_add',name:name.trim(),installPath:root.trim(),executable:resolveGameExecutablePath(executable.trim(),root.trim()),...(imagePath.trim()?{imagePath:imagePath.trim()}:{})};
}

export function resolveGameExecutablePath(path: string, installationPath?: string | null): string {
  if (!path || !installationPath || /^(?:[a-z]:[\\/]|[\\/])/i.test(path)) return path;
  const installation = gameDisplayPath(installationPath);
  const windows = /^[a-z]:[\\/]/i.test(installation) || installation.startsWith('\\\\');
  const root = installation.replace(windows ? /[\\/]+$/ : /\/+$/, '');
  return root + (windows ? '\\' : '/') + (windows ? path.replaceAll('/', '\\') : path);
}

export function gameIntegrationLabel(game: Game): string {
  if (!gameHasIntegration(game)) return 'Bindings and profile activation';
  if (!game.installed) return 'Game not installed';
  if (!game.integrationSetup) return 'Integration not checked';
  if (game.provider === 'assetto-corsa-rally' && game.integrationSetup.state === 'ready' && game.integrationSetup.connection?.source === 'synthetic') return 'Synthetic test telemetry configured';
  if (sharedMemoryGame(game) && ['ready', 'needs-attention'].includes(game.integrationSetup.state) && game.integrationSetup.connection?.source === 'shared-memory' && !gameIntegrationNativeVerified(game)) return 'Telemetry configured · verification needed';
  if (sharedMemoryGame(game) && game.integrationSetup.state === 'ready' && !gameIntegrationNativeVerified(game)) return 'Native telemetry not verified';
  const labels: Record<GameIntegrationSetup['state'], string> = {
    'not-installed': 'Game not installed', 'setup-required': 'Integration setup required',
    'setting-up': 'Setting up integration', ready: 'Integration ready',
    'needs-attention': 'Integration needs attention', 'unsupported-platform': 'Setup unavailable on this platform',
  };
  return labels[game.integrationSetup.state];
}

export function gameIntegrationNativeVerified(game: Game): boolean {
  return sharedMemoryGame(game) && game.integrationSetup?.verified === true && game.integrationSetup.connection?.source === 'shared-memory';
}

export function gameIntegrationReady(game: Game): boolean {
  return game.installed && game.integrationSetup?.state === 'ready' && (game.provider === 'dcs-bios' || game.provider === 'x-plane' || gameIntegrationNativeVerified(game));
}

export function gameIntegrationInstalled(game: Game): boolean {
  return game.integrationSetup?.installed ?? ['ready','needs-attention'].includes(game.integrationSetup?.state??'');
}

export function gameIntegrationVerificationLabel(game: Game): string {
  return gameIntegrationNativeVerified(game) ? 'Native telemetry verified' : 'Native telemetry not verified';
}

export function gameIntegrationLiveLabel(game: Game): string {
  if (game.provider === 'assetto-corsa-rally' && game.integrationSetup?.connection?.source === 'synthetic') return game.integrationSetup.live === 'receiving' ? 'Receiving synthetic test data' : game.integrationSetup.live === 'stale' ? 'Synthetic test data is stale' : 'Waiting for synthetic test data';
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
  return game.provider === 'dcs-bios' ? gameIntegrationFolderError(folder) : '';
}

export function gameIntegrationSetupRequest(game: Game, command: 'game_integration_check' | 'game_integration_setup' | 'game_integration_cancel' | 'game_integration_recover' | 'game_integration_remove', folder: string): GameRequest {
  return { command, gameId: game.id, ...(game.provider === 'dcs-bios' && folder.trim() ? { userFolder: folder.trim() } : {}) };
}

export function gameIntegrationSetupBlockedReason(game: Game, profileBlockedReason: string): string {
  if (!game.installed) return 'Install the game or select its executable first.';
  if (game.integrationSetup?.state === 'unsupported-platform') return game.integrationSetup.message;
  if (game.integrationSetup?.state === 'setting-up') return 'Integration setup is in progress.';
  if (game.integrationSetup?.recoveryNeeded) return 'Recover the interrupted setup before trying again.';
  if (game.provider === 'dcs-bios' && game.integrationSetup?.repairable === false) return game.integrationSetup.message;
  if (profileBlockedReason) return profileBlockedReason;
  return '';
}
