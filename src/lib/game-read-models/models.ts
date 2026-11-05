import type { Game, GamesState } from '../games.ts';
import type { DeepReadonly } from '../profiles.ts';
import type { WireFrame } from '../streams/protocol.ts';
import { digestId, record, valid } from './pages.ts';

export type GameSummary = Omit<Game, 'vehicleSupport'> & {
  detailsRevision?: string;
  vehicleSupport?: Omit<NonNullable<Game['vehicleSupport']>, 'vehicles'> & { vehicleCount: number };
};
export type GamesSummaryState = Omit<GamesState, 'catalogue'> & { catalogue: GameSummary[] };
export type GameDetailModel = { gameId: string; scope: string; value: Game };
export type GameJob = { key: string; run: string; phase: string; message: string; percent?: number; terminal: boolean; truncated?: boolean };
export type GameJobsModel = { jobs: GameJob[] };
export type GameModelState<T> = Readonly<{
  status: 'connecting' | 'ready' | 'lost' | 'error'; value?: DeepReadonly<T>;
  generation?: string; revision?: string; clockId?: string; session?: string; error?: string;
  resolvingPage?: boolean;
}>;
export type GameListener<T> = (state: GameModelState<T>) => void;
const gameId = (value: unknown): value is string => typeof value === 'string' && /^[a-z0-9][a-z0-9-]{0,95}$/.test(value);
const text = (value: unknown): value is string => typeof value === 'string';
const optionalText = (value: unknown) => value === undefined || value === null || text(value);
const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);
const count = (value: unknown): value is number => Number.isSafeInteger(value) && Number(value) >= 0;
function strings(value: unknown): boolean { return Array.isArray(value) && value.every(text); }
function background(value: unknown): void {
  if (value === undefined || value === null) return;
  valid(record(value) && text(value.image) && finite(value.positionX) && finite(value.positionY));
  if (value.crop !== undefined) { const crop = value.crop; valid(record(crop) && ['x', 'y', 'width', 'height'].every(key => finite(crop[key]))); }
}
function vehicle(value: unknown): boolean { return record(value) && text(value.id) && text(value.name); }
function game(value: unknown, summary: boolean): asserts value is GameSummary | Game {
  valid(record(value) && gameId(value.id) && text(value.name) && typeof value.installed === 'boolean' && typeof value.launchSupported === 'boolean');
  for (const key of ['executable', 'manualPath', 'installPath', 'source', 'integrationKey', 'selectedInstallationId', 'launchReason', 'launchUnavailableReason']) valid(optionalText(value[key]));
  valid(value.provider === undefined || value.provider === null || ['dcs-bios', 'assetto-corsa-rally', 'assetto-corsa', 'assetto-corsa-competizione', 'assetto-corsa-evo', 'x-plane'].includes(value.provider as string));
  valid(value.tagIds === undefined || strings(value.tagIds));
  background(value.background); background(value.defaultBackground);
  if (value.vehicleSupport !== undefined) {
    const support = value.vehicleSupport;
    valid(record(support) && typeof support.supported === 'boolean' && ['aircraft', 'car'].includes(support.kind as string) && optionalText(support.message));
    if (summary) valid(!Object.hasOwn(support, 'vehicles') && count(support.vehicleCount));
    else valid(Array.isArray(support.vehicles) && support.vehicles.every(vehicle));
  }
  valid(value.currentVehicle === undefined || value.currentVehicle === null || vehicle(value.currentVehicle));
  if (value.installations !== undefined) valid(Array.isArray(value.installations) && value.installations.every(item => record(item) && text(item.id) && text(item.installPath)
    && text(item.source) && typeof item.available === 'boolean' && strings(item.executableCandidates) && optionalText(item.executable)));
  if (value.integrationSetup !== undefined) valid(record(value.integrationSetup) && text(value.integrationSetup.message)
    && ['checking', 'unavailable', 'not-installed', 'setup-required', 'setting-up', 'ready', 'needs-attention', 'unsupported-platform'].includes(value.integrationSetup.state as string));
  if (summary) valid(value.detailsRevision === undefined || digestId(value.detailsRevision));
}
/** Check the small, issued envelope before following any references. */
export function captureEnvelope(frame: WireFrame, topic: string): Record<string, unknown> {
  valid(frame.topic === topic && frame.source === 'games' && /^games:[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(frame.clockId)
    && frame.kind !== 'loss' && record(frame.payload));
  const payload = frame.payload;
  if (topic === 'games.state') valid(Object.keys(payload).length === 1 && Object.hasOwn(payload, 'value'));
  else if (topic.startsWith('games.detail.')) valid(Object.keys(payload).length === 3 && payload.gameId === topic.slice('games.detail.'.length)
    && gameId(payload.gameId) && digestId(payload.scope) && Object.hasOwn(payload, 'value'));
  else valid(topic === 'games.jobs' && Object.keys(payload).length === 1 && Array.isArray(payload.jobs));
  return payload;
}
/** Already decoded immutable nodes are returned directly, preserving hash-based identity. */
export function captureGamesState(value: unknown): DeepReadonly<GamesSummaryState> {
  valid(record(value) && Array.isArray(value.catalogue));
  const ids = new Set<string>();
  for (const item of value.catalogue) { game(item, true); valid(!ids.has(item.id)); ids.add(item.id); }
  if (value.tags !== undefined) valid(record(value.tags) && Object.values(value.tags).every(text));
  if (value.detectedGames !== undefined) valid(Array.isArray(value.detectedGames) && value.detectedGames.every(item => record(item) && gameId(item.gameId) && text(item.name)));
  valid(value.detectionErrors === undefined || strings(value.detectionErrors));
  valid(value.artworkPending === undefined || typeof value.artworkPending === 'boolean');
  valid(value.addedGameId === undefined || value.addedGameId === null || gameId(value.addedGameId)); valid(optionalText(value.error));
  if (value.discovery !== undefined) {
    const discovery = value.discovery;
    valid(record(discovery) && text(discovery.scanId) && ['idle', 'scanning', 'ready', 'applied', 'failed'].includes(discovery.status as string)
      && Array.isArray(discovery.libraries) && Array.isArray(discovery.entries) && strings(discovery.diagnostics) && typeof discovery.firstStart === 'boolean');
    valid(discovery.entries.every(entry => record(entry) && text(entry.id) && text(entry.name) && text(entry.library) && text(entry.installPath)
      && ['add', 'update', 'remove'].includes(entry.action as string) && typeof entry.selected === 'boolean' && strings(entry.executableCandidates)));
  }
  if (value.backgroundPreview !== undefined) valid(record(value.backgroundPreview) && text(value.backgroundPreview.image));
  return value as DeepReadonly<GamesSummaryState>;
}
export function captureGameDetail(payload: Record<string, unknown>, value: unknown): DeepReadonly<GameDetailModel> {
  game(value, false); valid(value.id === payload.gameId);
  return Object.freeze({ gameId: payload.gameId as string, scope: payload.scope as string, value }) as DeepReadonly<GameDetailModel>;
}
export function captureGameJobs(payload: Record<string, unknown>): DeepReadonly<GameJobsModel> {
  valid(Array.isArray(payload.jobs) && payload.jobs.length <= 16);
  const keys = new Set<string>();
  const jobs = payload.jobs.map(job => {
    valid(record(job) && Object.keys(job).every(key => ['key', 'run', 'phase', 'message', 'percent', 'terminal', 'truncated'].includes(key))
      && text(job.key) && job.key.length > 0 && job.key.length <= 64 && text(job.run) && job.run.length > 0 && job.run.length <= 128
      && text(job.phase) && job.phase.length > 0 && job.phase.length <= 64 && text(job.message) && job.message.length <= 1024 && typeof job.terminal === 'boolean'
      && (job.percent === undefined || (finite(job.percent) && job.percent >= 0 && job.percent <= 100))
      && (job.truncated === undefined || typeof job.truncated === 'boolean') && !keys.has(job.key));
    keys.add(job.key); return Object.freeze({ ...job });
  });
  return Object.freeze({ jobs: Object.freeze(jobs) }) as DeepReadonly<GameJobsModel>;
}
