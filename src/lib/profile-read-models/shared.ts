import { SharedObservations, type ObservationState } from '../streams/shared.ts';
import { captureLibrary, captureRuntime, runtimeRulesManifest, withRuntimeRules, type LibraryModel, type RuntimeModel, type ModelState } from './models.ts';
import { ProfileRulePages, type ReadModelPageGetter } from './rule-pages.ts';
import type { DeepReadonly } from '../profiles.ts';
import type { WireFrame } from '../streams/protocol.ts';

type Listener<T> = (state: ModelState<T>) => void;
interface Resolution { key: string; controller: AbortController; }
interface Entry<T> {
  state: ModelState<T>; listeners: Set<Listener<T>>; stop?: () => void; clockId?: string;
  continuous: boolean; failedResolution?: string; pendingFrame?: WireFrame; resolution?: Resolution;
}
async function nativePage(id: string, offset: number): Promise<unknown> {
  const {gatewayRequest} = await import('../gateway.ts');
  return (await gatewayRequest({command:'profile_read_model_page',id,offset})).profileReadModelPage;
}
/** One validated immutable projection and transport registration per exact topic. */
export class ProfileReadModels {
  private library?: Entry<LibraryModel>;
  private runtime?: Entry<RuntimeModel>;
  private pages: ProfileRulePages;
  constructor(private observations = new SharedObservations(), getPage: ReadModelPageGetter = nativePage) { this.pages = new ProfileRulePages(getPage); }
  observeLibrary(listener: Listener<LibraryModel>): () => void { return this.observe('library', 'profiles.library', captureLibrary, listener); }
  observeRuntime(listener: Listener<RuntimeModel>): () => void { return this.observe('runtime', 'profiles.runtime', captureRuntime, listener); }
  private observe<T extends LibraryModel|RuntimeModel>(key: 'library'|'runtime', topic: string, capture: (frame: WireFrame) => DeepReadonly<T>, listener: Listener<T>): () => void {
    let entry = this[key] as Entry<T>|undefined;
    const fresh = !entry;
    if (!entry) { entry = {state:Object.freeze({status:'connecting'}),listeners:new Set(),continuous:false}; (this as unknown as Record<string, unknown>)[key] = entry; }
    const current = entry;
    const active = () => this[key] === current && current.listeners.size > 0;
    const publish = () => { for (const next of [...current.listeners]) if (active() && current.listeners.has(next)) this.notify(next,current.state); };
    const registration: Listener<T> = state => listener(state);
    current.listeners.add(registration);this.notify(registration,current.state);
    if (fresh && active()) {
      const stop = this.observations.subscribe(topic,state => {
        if (active()) this.accept(current,state,capture,active,publish,key === 'runtime');
      });
      if (active()) current.stop = stop; else stop();
    }
    let subscribed = true;
    return () => {
      if (!subscribed) return;subscribed=false;current.listeners.delete(registration);
      if (!current.listeners.size && this[key] === current) { delete this[key];this.cancel(current);current.stop?.(); }
    };
  }
  private notify<T>(listener: Listener<T>, state: ModelState<T>) { try { listener(state); } catch { /* Isolate consumers and preserve delivery acknowledgment. */ } }
  private cancel<T>(entry: Entry<T>): void { entry.resolution?.controller.abort();entry.resolution=undefined;entry.pendingFrame=undefined; }
  private ready<T>(entry: Entry<T>, frame: WireFrame, capture: (frame: WireFrame) => DeepReadonly<T>): void {
    const value = capture(frame);
    entry.clockId=frame.clockId;entry.continuous=true;
    entry.state=Object.freeze({status:'ready',value,generation:frame.generation,revision:frame.revision,clockId:frame.clockId,session:frame.session});
  }
  private fail<T>(entry: Entry<T>, error: unknown): void {
    this.cancel(entry);entry.continuous=false;
    entry.state=Object.freeze({...entry.state,status:'error',error:error instanceof Error?error.message:String(error)});
  }
  private pageFailure<T>(entry: Entry<T>, key: string, error: unknown): void {
    this.cancel(entry);entry.failedResolution=key;
    // The complete wire replacement remains ordered; page expiry is not wire loss.
    entry.state=Object.freeze({...entry.state,status:'error',error:error instanceof Error?error.message:String(error)});
  }
  private accept<T>(entry: Entry<T>, state: ObservationState, capture: (frame: WireFrame) => DeepReadonly<T>, active: () => boolean, publish: () => void, runtime: boolean): void {
    if (state.status !== 'ready') {
      this.cancel(entry);entry.continuous=false;entry.state=Object.freeze({...entry.state,status:state.status,error:state.error});publish();return;
    }
    const frame=state.frame!;
    try {
      if (entry.clockId && frame.clockId !== entry.clockId) throw new Error('Profile producer clock changed. Reopen the observation.');
      if (!entry.continuous && frame.kind !== 'snapshot') {
        entry.state=Object.freeze({...entry.state,status:'lost',error:'Waiting for a fresh profile baseline.'});publish();return;
      }
      const manifest=runtime?runtimeRulesManifest(frame):undefined;
      if (!manifest) { entry.failedResolution=undefined;this.cancel(entry);this.ready(entry,frame,capture);publish();return; }
      const cached=this.pages.cached(manifest);
      if (cached) { entry.failedResolution=undefined;this.cancel(entry);this.ready(entry,withRuntimeRules(frame,cached),capture);publish();return; }
      const key=JSON.stringify([frame.clockId,frame.session,frame.generation,manifest.id,manifest.bytes,manifest.count]);
      if(entry.failedResolution===key && frame.kind!=='snapshot') return;
      entry.failedResolution=undefined;
      entry.pendingFrame=frame;entry.continuous=true;entry.clockId=frame.clockId;
      if (entry.resolution?.key === key) return;
      this.cancel(entry);entry.pendingFrame=frame;
      const resolution: Resolution={key,controller:new AbortController()};entry.resolution=resolution;
      entry.state=Object.freeze({...entry.state,status:'lost',error:'Loading current automatic-profile rules.'});publish();
      if (!active() || resolution.controller.signal.aborted) return;
      void this.pages.resolve(manifest,resolution.controller.signal).then(rules => {
        if (!active() || entry.resolution !== resolution || !entry.pendingFrame) return;
        const latest=entry.pendingFrame;
        try { this.ready(entry,withRuntimeRules(latest,rules),capture);this.cancel(entry); }
        catch (error) { this.pageFailure(entry,key,error); }
        publish();
      },error => { if (active() && entry.resolution === resolution) { this.pageFailure(entry,key,error);publish(); } });
    } catch (error) { this.fail(entry,error);publish(); }
  }
}
let shared: ProfileReadModels|undefined;
export function observeProfileLibrary(listener: Listener<LibraryModel>): () => void { return (shared ??= new ProfileReadModels()).observeLibrary(listener); }
export function observeProfileRuntime(listener: Listener<RuntimeModel>): () => void { return (shared ??= new ProfileReadModels()).observeRuntime(listener); }
