import { gameIntegrationFolderError, type Game, type GameId, type GameRequest } from './games';

export type DcsConnection = { cataloguePath: string; freshnessMs: number; commandHost: string; commandPort: number; listenAddress: string; listenPort: number; multicastGroup: string; multicastInterface: string };
export type XPlaneConnection = { source: 'rref'; version: 11 | 12; host: string; port: number; listenPort: number; updateHz: number; freshnessMs: number };
export type RallyConnection = { source: 'shared-memory'; freshnessMs: number };
export type ProviderConnection = DcsConnection | XPlaneConnection | RallyConnection;
export type ProviderSettingsDraft = { gameId: GameId; baseRevision: string | null; connection: ProviderConnection; userFolder: string; aircraftModule: string };
export type ProviderSettingsSave = { gameId: GameId; baseRevision: string; submitted: ProviderSettingsDraft; request: GameRequest };
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));
const integer = (value: unknown, min: number, max: number): boolean => typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max;
const string = (value: unknown, max: number, empty = false): value is string => typeof value === 'string' && (empty || !!value.trim()) && new TextEncoder().encode(value).length <= max && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
const ipv4 = (value: unknown): value is string => typeof value === 'string' && value.split('.').length === 4 && value.split('.').every(part => /^(0|[1-9]\d{0,2})$/.test(part) && Number(part) <= 255);
const multicast = (value: string): boolean => Number(value.split('.')[0]) >= 224 && Number(value.split('.')[0]) <= 239;
const unicast = (value: unknown, allowUnspecified: boolean): boolean => ipv4(value) && !multicast(value) && value !== '255.255.255.255' && (allowUnspecified || value !== '0.0.0.0');
const revision = (value: unknown): value is string => typeof value === 'string' && /^sha256:[0-9a-f]{64}$/.test(value);
const keys = (value: object, expected: string[]) => Object.keys(value).sort().join(',') === expected.sort().join(',');

export function defaultProviderConnection(gameId: GameId): ProviderConnection {
  if (gameId === 'dcs') return { cataloguePath: '', freshnessMs: 1500, commandHost: '127.0.0.1', commandPort: 7778, listenAddress: '0.0.0.0', listenPort: 5010, multicastGroup: '239.255.50.10', multicastInterface: '0.0.0.0' };
  if (gameId === 'assetto-corsa-rally') return { source: 'shared-memory', freshnessMs: 1500 };
  return { source: 'rref', version: gameId === 'x-plane-12' ? 12 : 11, host: '127.0.0.1', port: 49000, listenPort: 0, updateHz: 10, freshnessMs: 1500 };
}

/** Project only editable saved provider settings. Process identities and routing never enter a draft. */
export function providerSettingsDraft(game: Game): ProviderSettingsDraft {
  const connection = defaultProviderConnection(game.id);
  const saved = game.integrationSetup?.connection ?? {};
  for (const key of Object.keys(connection)) if (key in saved) (connection as unknown as Record<string, unknown>)[key] = saved[key];
  return { gameId: game.id, baseRevision: revision(game.integrationSetup?.providerSetupRevision) ? game.integrationSetup.providerSetupRevision : null, connection: clone(connection), userFolder: game.integrationSetup?.userFolder ?? '', aircraftModule: game.integrationSetup?.aircraftModule ?? '' };
}

export function providerSettingsError(draft: ProviderSettingsDraft): string {
  if (!revision(draft.baseRevision)) return 'Refresh game settings before saving; their setup revision is unavailable.';
  const c = draft.connection;
  if (!integer(c.freshnessMs, 100, 10000)) return 'Telemetry freshness must be a whole number from 100 to 10000 ms.';
  if (draft.gameId === 'dcs') {
    if (!keys(c, ['cataloguePath','freshnessMs','commandHost','commandPort','listenAddress','listenPort','multicastGroup','multicastInterface'])) return 'Use only the DCS provider connection settings.';
    const d = c as DcsConnection;
    if (!string(d.cataloguePath, 4096) || gameIntegrationFolderError(d.cataloguePath)) return 'Choose the absolute installed DCS-BIOS doc/json directory.';
    const folderError = gameIntegrationFolderError(draft.userFolder); if (folderError) return folderError;
    if (!string(draft.userFolder, 4096) || !string(draft.aircraftModule, 63, true) || draft.aircraftModule !== '' && !/^[A-Za-z0-9_-]+$/.test(draft.aircraftModule)) return 'Use an absolute Saved Games folder and an aircraft module of up to 63 letters, numbers, underscores or hyphens.';
    if (!unicast(d.commandHost,false) || !unicast(d.listenAddress,true) || !unicast(d.multicastInterface,true) || d.multicastGroup !== '' && (!ipv4(d.multicastGroup) || !multicast(d.multicastGroup))) return 'Use unicast IPv4 command/listen/interface addresses and a multicast group in 224.0.0.0–239.255.255.255, or leave the group empty. Command host cannot be 0.0.0.0.';
    if (!integer(d.commandPort,1,65535) || !integer(d.listenPort,1,65535)) return 'DCS UDP ports must be whole numbers from 1 to 65535.';
  } else if (draft.gameId === 'assetto-corsa-rally') {
    if (!keys(c, ['source','freshnessMs']) || (c as RallyConnection).source !== 'shared-memory') return 'Rally uses native shared-memory telemetry; process identity is verified by Cockpit.';
  } else {
    if (!keys(c, ['source','version','host','port','listenPort','updateHz','freshnessMs']) || (c as XPlaneConnection).source !== 'rref') return 'Use only the X-Plane RREF provider connection settings.';
    const x = c as XPlaneConnection;
    if (x.version !== (draft.gameId === 'x-plane-12' ? 12 : 11)) return 'The connection version must match this X-Plane game.';
    if (!ipv4(x.host) || Number(x.host.split('.')[0]) !== 127) return 'Use a local IPv4 loopback address (127.x.x.x) for the monitored X-Plane installation.';
    if (!integer(x.port,1,65535) || !integer(x.listenPort,0,65535) || !integer(x.updateHz,1,20) || x.freshnessMs < 2000 / x.updateHz) return 'Choose valid UDP ports, 1–20 updates per second and freshness allowing two updates.';
  }
  return '';
}

export function providerSettingsEqual(a: ProviderSettingsDraft, b: ProviderSettingsDraft): boolean {
  return JSON.stringify({ ...a, baseRevision: null }) === JSON.stringify({ ...b, baseRevision: null });
}

export function prepareProviderSettingsSave(draft: ProviderSettingsDraft): ProviderSettingsSave {
  const error = providerSettingsError(draft); if (error) throw new Error(error);
  const submitted = clone(draft);
  return { gameId: draft.gameId, baseRevision: draft.baseRevision!, submitted, request: { command: 'game_integration_settings_save', gameId: draft.gameId, expectedSetupRevision: draft.baseRevision!, connection: clone(draft.connection) as unknown as Record<string, unknown>, ...(draft.gameId === 'dcs' ? { userFolder: draft.userFolder.trim(), aircraftModule: draft.aircraftModule } : {}) } };
}

/** Advance only the captured editor base. Newer edits remain local; a switched editor is untouched. */
export function settleProviderSettingsSave(current: ProviderSettingsDraft, save: ProviderSettingsSave, game: Game): ProviderSettingsDraft {
  if (current.gameId !== save.gameId || game.id !== save.gameId || current.baseRevision !== save.baseRevision) return current;
  const saved = providerSettingsDraft(game);
  if (!saved.baseRevision) throw new Error('The saved game settings did not include a setup revision. Refresh before saving again.');
  return providerSettingsEqual(current, save.submitted) ? saved : { ...current, baseRevision: saved.baseRevision };
}

export function gameContextLabel(game: Game): string {
  if (!game.integrationContext) return 'Game context not verified';
  const c = game.integrationContext;
  if (c.status === 'ready' && c.processVerified) return 'Matching game context verified';
  const labels = { 'waiting-profile': 'Waiting for an active profile', 'waiting-game': 'Waiting for the matching game', 'setup-required': 'Provider setup required', 'unsupported-platform': 'Game context unavailable on this platform', error: 'Game context needs attention', ready: 'Game context not verified' };
  return labels[c.status];
}
