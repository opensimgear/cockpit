import { SharedObservations, type ObservationState } from '../streams/shared.ts';
import type { WireFrame } from '../streams/protocol.ts';
import type { DeepReadonly } from '../profiles.ts';
import { claimObservedGameCatalogue, updateGameCatalogue } from '../games.ts';
import type { Game } from '../games.ts';
import { assetReference, GamePages, valid, type GamePageGetter } from './pages.ts';
import { captureEnvelope, captureGameDetail, captureGameJobs, captureGamesState, type GameDetailModel, type GameJobsModel,
  type GameListener, type GameModelState, type GamesSummaryState } from './models.ts';
export type { GameDetailModel, GameJobsModel, GameJob, GameModelState, GamesSummaryState, GameSummary } from './models.ts';
export type { GamePageGetter } from './pages.ts';

interface Resolution { key: string; controller: AbortController; frame: WireFrame; }
interface Entry<T> {
  state: GameModelState<T>; listeners: Set<GameListener<T>>; stop?: () => void; resolution?: Resolution;
  continuous: boolean; latest?: WireFrame; releaseCatalogue?: () => void;
}
async function nativePage(id: string, offset: number, signal: AbortSignal): Promise<unknown> {
  if (signal.aborted) throw new Error('Game read-model request cancelled.');
  const { gatewayRequest } = await import('../gateway.ts');
  if (signal.aborted) throw new Error('Game read-model request cancelled.');
  return (await gatewayRequest({ command: 'game_read_model_page', id, offset })).gameReadModelPage;
}
/** One immutable projection, resolution and transport registration for each exact topic. */
export class GameReadModels {
  private entries = new Map<string, Entry<unknown>>();
  private pages: GamePages;
  constructor(private observations: Pick<SharedObservations, 'subscribe'> = new SharedObservations(), getPage: GamePageGetter = nativePage) { this.pages = new GamePages(getPage); }
  observeGamesState(listener: GameListener<GamesSummaryState>): () => void { return this.observe('games.state', listener); }
  observeGameDetails(gameId: string, listener: GameListener<GameDetailModel>): () => void {
    if (!/^[a-z0-9][a-z0-9-]{0,95}$/.test(gameId)) { this.notify(listener, { status: 'error', error: 'Choose an exact game.' }); return () => {}; }
    return this.observe(`games.detail.${gameId}`, listener);
  }
  observeGameJobs(listener: GameListener<GameJobsModel>): () => void { return this.observe('games.jobs', listener); }
  observeGameAsset(reference: string, listener: GameListener<string>): () => void {
    try { assetReference(reference); }
    catch (error) { this.notify(listener, { status: 'error', error: this.message(error) }); return () => {}; }
    return this.observe(reference, listener, true);
  }
  private observe<T>(topic: string, listener: GameListener<T>, asset = false): () => void {
    let entry = this.entries.get(topic) as Entry<T> | undefined;
    const fresh = !entry;
    if (!entry) { entry = { state: Object.freeze({ status: 'connecting' }), listeners: new Set(), continuous: false,
      ...(topic === 'games.state' ? { releaseCatalogue: claimObservedGameCatalogue() } : {}) }; this.entries.set(topic, entry as Entry<unknown>); }
    const current = entry;
    const active = () => this.entries.get(topic) === current && current.listeners.size > 0;
    const publish = () => { for (const next of [...current.listeners]) if (active() && current.listeners.has(next)) this.notify(next, current.state); };
    const registration: GameListener<T> = state => listener(state);
    current.listeners.add(registration); this.notify(registration, current.state);
    if (fresh && active()) {
      if (asset) {
        const controller = new AbortController();
        current.stop = () => controller.abort();
        void this.pages.resolveAsset(assetReference(topic), controller.signal).then(value => {
          if (active() && !controller.signal.aborted) { current.state = Object.freeze({ status: 'ready', value: value as DeepReadonly<T> }); publish(); }
        }, error => { if (active()) { this.fail(current, error); publish(); } });
      } else {
        try {
          const stop = this.observations.subscribe(topic, state => { if (active()) this.accept(topic, current, state, active, publish); });
          if (active()) current.stop = stop; else stop();
        } catch (error) { this.fail(current, error); publish(); }
      }
    }
    let subscribed = true;
    return () => {
      if (!subscribed) return; subscribed = false; current.listeners.delete(registration);
      if (!current.listeners.size && this.entries.get(topic) === current) { this.entries.delete(topic); this.cancel(current); current.stop?.(); current.releaseCatalogue?.(); }
    };
  }
  private notify<T>(listener: GameListener<T>, state: GameModelState<T>): void { try { listener(state); } catch { /* Isolate consumers and preserve transport acknowledgment. */ } }
  private message(error: unknown): string { return error instanceof Error ? error.message : String(error); }
  private cancel<T>(entry: Entry<T>): void { const pending = entry.resolution; entry.resolution = undefined; pending?.controller.abort(); }
  private fail<T>(entry: Entry<T>, error: unknown): void {
    this.cancel(entry); entry.continuous = false;
    entry.state = Object.freeze({ ...entry.state, status: 'error', error: this.message(error) });
  }
  private pageFailure<T>(entry: Entry<T>, error: unknown): void {
    this.cancel(entry);
    // A complete wire replacement stays ordered when its immutable page expires.
    entry.state = Object.freeze({ ...entry.state, status: 'error', error: this.message(error) });
  }
  private ready<T>(entry: Entry<T>, frame: WireFrame, value: unknown): void {
    entry.state = Object.freeze({ status: 'ready', value: value as DeepReadonly<T>, generation: frame.generation, revision: frame.revision, clockId: frame.clockId, session: frame.session });
  }
  private accept<T>(topic: string, entry: Entry<T>, state: ObservationState, active: () => boolean, publish: () => void): void {
    if (state.status !== 'ready') {
      this.cancel(entry); entry.continuous = false;
      entry.state = Object.freeze({ ...entry.state, status: state.status, error: state.error }); publish(); return;
    }
    try {
      const frame = state.frame!;
      const payload = captureEnvelope(frame, topic);
      const previous = entry.latest;
      valid(!previous || (previous.clockId === frame.clockId && previous.session === frame.session));
      if (previous && (BigInt(frame.generation) < BigInt(previous.generation)
        || (frame.generation === previous.generation && (BigInt(frame.revision) < BigInt(previous.revision)
          || (frame.kind !== 'snapshot' && frame.revision === previous.revision))))) return;
      if ((!entry.continuous || (previous && previous.generation !== frame.generation)) && frame.kind !== 'snapshot') {
        this.cancel(entry); entry.continuous = false;
        entry.state = Object.freeze({ ...entry.state, status: 'lost', error: 'Waiting for a fresh game baseline.' }); publish(); return;
      }
      entry.latest = frame; entry.continuous = true;
      if (topic === 'games.jobs') { this.cancel(entry); this.ready(entry, frame, captureGameJobs(payload)); publish(); return; }
      const key = JSON.stringify([frame.clockId, frame.session, frame.generation, payload]);
      if (entry.resolution?.key === key) { entry.resolution.frame = frame; return; }
      this.cancel(entry);
      const resolution: Resolution = { key, controller: new AbortController(), frame }; entry.resolution = resolution;
      entry.state = Object.freeze({ ...entry.state, status: 'lost', error: 'Loading current game data.' }); publish();
      if (!active() || resolution.controller.signal.aborted) return;
      void this.pages.resolve(payload.value, resolution.controller.signal).then(value => {
        if (!active() || entry.resolution !== resolution) return;
        try {
          const result = topic === 'games.state' ? captureGamesState(value) : captureGameDetail(payload, value);
          if (topic === 'games.state') updateGameCatalogue((result as DeepReadonly<GamesSummaryState>).catalogue as readonly Game[]);
          this.ready(entry, resolution.frame, result); this.cancel(entry);
        } catch (error) { this.fail(entry, error); }
        publish();
      }, error => { if (active() && entry.resolution === resolution) { this.pageFailure(entry, error); publish(); } });
    } catch (error) { this.fail(entry, error); publish(); }
  }
}
let shared: GameReadModels | undefined;
export function observeGamesState(listener: GameListener<GamesSummaryState>): () => void { return (shared ??= new GameReadModels()).observeGamesState(listener); }
export function observeGameDetails(gameId: string, listener: GameListener<GameDetailModel>): () => void { return (shared ??= new GameReadModels()).observeGameDetails(gameId, listener); }
export function observeGameJobs(listener: GameListener<GameJobsModel>): () => void { return (shared ??= new GameReadModels()).observeGameJobs(listener); }
export function observeGameAsset(reference: string, listener: GameListener<string>): () => void { return (shared ??= new GameReadModels()).observeGameAsset(reference, listener); }
