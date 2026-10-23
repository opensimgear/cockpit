import { createBehaviorState, reduceBehavior, type BehaviorEvent, type BehaviorEffect, type BehaviorState, type Scalar } from './behavior.ts';
import { decodeWidgetBase64, validatePcmWave, type CompiledWidgetArtifact, type CompiledWidgetBundle, type CompiledMotion, type CompiledSound } from './artifact.ts';

/** Static frames are selected by finite samples and region states, never by authoring layers. */
export function selectCompiledFrame(artifact: CompiledWidgetArtifact, state: Pick<BehaviorState, 'values' | 'regions'>): string {
  const values: Record<string, Scalar> = {};
  for (const domain of artifact.domains) {
    const value = state.values[domain.signal];
    if (domain.numeric && typeof value === 'number') {
      let nearest = domain.values[0] as number;
      for (const sample of domain.values as number[]) if (Math.abs(sample - value) < Math.abs(nearest - value)) nearest = sample;
      values[domain.signal] = nearest;
    } else values[domain.signal] = domain.values.includes(value) ? value : domain.values[0];
  }
  const frame = artifact.frames.find(frame => frame.selector && Object.entries(values).every(([key, value]) => frame.selector!.values[key] === value) && Object.entries(frame.selector.regions).every(([key, value]) => state.regions[key] === value));
  if (!frame) throw new Error('No compiled presentation matches the current behavior state.');
  return frame.id;
}

interface Playback { motion: CompiledMotion; startedAt: number; start: number; end: number; duration: number }
interface ScalarPlayback { start: number; target: number; current: number; startedAt: number; duration: number }
export interface PlayerUpdate { state: BehaviorState; effects: BehaviorEffect[]; frameId: string; error?: string }

/** The behavior reducer completes synchronously before any decorative playback begins. */
export class CompiledWidgetPlayer {
  readonly artifact: CompiledWidgetArtifact;
  state: BehaviorState;
  private desired: string;
  private shown: string;
  private playback?: Playback;
  private motionEnabled = true;
  private visible = true;
  private scalars = new Map<string, ScalarPlayback>();
  constructor(bundle: CompiledWidgetBundle, nowMs = 0) {
    this.artifact = bundle.artifact;
    this.state = createBehaviorState(this.artifact.behavior, nowMs);
    this.desired = this.shown = selectCompiledFrame(this.artifact, this.state);
    this.presentation(nowMs);
  }
  get frameId(): string { return this.shown; }
  get animating(): boolean { return !!this.playback || [...this.scalars.values()].some(scalar => scalar.current !== scalar.target); }
  private presentation(nowMs: number): string {
    if (!this.motionEnabled || !this.visible) return selectCompiledFrame(this.artifact, this.state);
    const values = { ...this.state.values };
    for (const domain of this.artifact.domains) {
      if (!domain.numeric || !domain.smoothingMs) continue;
      const samples = domain.values as number[], raw = Number(values[domain.signal]);
      let target = 0;
      for (let i = 1; i < samples.length; i++) if (Math.abs(samples[i] - raw) < Math.abs(samples[target] - raw)) target = i;
      let scalar = this.scalars.get(domain.signal);
      if (!scalar) { scalar = { start: target, current: target, target, startedAt: nowMs, duration: domain.smoothingMs }; this.scalars.set(domain.signal, scalar); }
      scalar.current = scalar.start + (scalar.target - scalar.start) * Math.min(1, Math.max(0, (nowMs - scalar.startedAt) / scalar.duration));
      if (target !== scalar.target) { scalar.start = scalar.current; scalar.target = target; scalar.startedAt = nowMs; scalar.duration = domain.smoothingMs; }
      values[domain.signal] = samples[Math.max(0, Math.min(samples.length - 1, Math.round(scalar.current)))];
    }
    return selectCompiledFrame(this.artifact, { values, regions: this.state.regions });
  }
  private progress(nowMs: number): number {
    const p = this.playback!;
    return p.start + (p.end - p.start) * Math.min(1, Math.max(0, (nowMs - p.startedAt) / Math.max(1, p.duration)));
  }
  private present(nowMs: number): string {
    if (!this.playback) return this.shown = this.desired;
    const progress = this.progress(nowMs), frames = this.playback.motion.frames;
    this.shown = frames[Math.min(frames.length - 1, Math.max(0, Math.round(progress * (frames.length - 1))))];
    if (nowMs >= this.playback.startedAt + this.playback.duration) { this.playback = undefined; this.shown = this.desired; }
    return this.shown;
  }
  private target(target: string, nowMs: number): void {
    this.present(nowMs);
    if (target === this.desired) return;
    const oldDesired = this.desired, active = this.playback;
    this.desired = target;
    if (!this.visible || !this.motionEnabled) { this.playback = undefined; this.shown = target; return; }
    if (active?.motion.interruption === 'reverse' && (target === active.motion.fromFrameId || target === active.motion.toFrameId)) {
      const start = this.progress(nowMs), end = target === active.motion.toFrameId ? 1 : 0;
      this.playback = { motion: active.motion, startedAt: nowMs, start, end, duration: Math.max(1, active.motion.durationMs * Math.abs(end - start)) };
      return;
    }
    const motion = this.artifact.motions.find(motion => motion.fromFrameId === (active ? oldDesired : this.shown) && motion.toFrameId === target);
    if (motion && (!active || motion.interruption === 'restart')) this.playback = { motion, startedAt: nowMs, start: 0, end: 1, duration: motion.durationMs };
    else { this.playback = undefined; this.shown = target; }
  }
  dispatch(event: BehaviorEvent): PlayerUpdate {
    const result = reduceBehavior(this.artifact.behavior, this.state, event);
    if (result.error) return { state: this.state, effects: [], frameId: this.shown, error: result.error };
    this.state = result.state;
    const frame = this.presentation(event.nowMs);
    if (event.type === 'reset') { this.playback = undefined; this.scalars.clear(); this.desired = this.shown = selectCompiledFrame(this.artifact, this.state); }
    else this.target(frame, event.nowMs);
    return { state: this.state, effects: result.effects, frameId: this.present(event.nowMs), ...(result.error ? { error: result.error } : {}) };
  }
  sample(nowMs: number): PlayerUpdate { return this.dispatch({ type: 'tick', nowMs }); }
  setMotionEnabled(enabled: boolean): void { this.motionEnabled = enabled; if (!enabled) this.snap(); }
  setVisible(visible: boolean): void { this.visible = visible; if (!visible) this.snap(); }
  private snap(): void { this.playback = undefined; this.scalars.clear(); this.shown = this.desired = selectCompiledFrame(this.artifact, this.state); }
}

export interface AudioVoice { stop(): void; finished?: Promise<void> }
export interface AudioDriver { resume(): Promise<void>; play(bytes: Uint8Array, volume: number): Promise<AudioVoice>; close(): void }
export type AudioAvailability = 'unsupported' | 'locked' | 'available' | 'muted' | 'hidden' | 'blocked';

/** Cues are transient intents. Locked, muted or hidden playback never accumulates a queue. */
export class CompiledSoundPlayer {
  private unlocked = false;
  private muted = false;
  private visible = true;
  private blocked = false;
  private closed = false;
  private epoch = 0;
  private exitEpoch = 0;
  private pending = 0;
  private last = new Map<string, number>();
  private voices: { sound: CompiledSound; voice: AudioVoice }[] = [];
  private bundle: CompiledWidgetBundle;
  private driver?: AudioDriver;
  constructor(bundle: CompiledWidgetBundle, driver?: AudioDriver) { this.bundle = bundle; this.driver = driver; }
  get availability(): AudioAvailability { return !this.driver ? 'unsupported' : this.muted ? 'muted' : !this.visible ? 'hidden' : this.blocked ? 'blocked' : !this.unlocked ? 'locked' : 'available'; }
  async unlock(): Promise<void> {
    if (!this.driver || this.closed) return;
    try { await this.driver.resume(); if (!this.closed) { this.unlocked = true; this.blocked = false; } }
    catch { this.blocked = true; }
  }
  private stop(all = true): void {
    if (all) this.epoch++; else this.exitEpoch++;
    for (const item of this.voices) if (all || item.sound.stopOnExit) item.voice.stop();
    this.voices = all ? [] : this.voices.filter(item => !item.sound.stopOnExit);
  }
  setMuted(muted: boolean): void { this.muted = muted; if (muted) this.stop(); }
  setVisible(visible: boolean): void { this.visible = visible; if (!visible) this.stop(); }
  exitState(): void { this.stop(false); }
  async cue(cue: string, nowMs: number): Promise<boolean> {
    if (this.closed || this.availability !== 'available' || !Number.isFinite(nowMs)) return false;
    const sound = this.bundle.artifact.sounds.find(sound => sound.cue === cue);
    if (!sound || nowMs - (this.last.get(sound.id) ?? -Infinity) < sound.cooldownMs || this.pending + this.voices.length >= 4) return false;
    const payload = this.bundle.assets.find(asset => asset.assetId === sound.assetId);
    if (!payload) return false;
    const epoch = this.epoch, exitEpoch = this.exitEpoch;
    this.last.set(sound.id, nowMs); this.pending++;
    try {
      const voice = await this.driver!.play(decodeWidgetBase64(payload.base64), sound.volume);
      if (epoch !== this.epoch || sound.stopOnExit && exitEpoch !== this.exitEpoch || this.closed || this.availability !== 'available') { voice.stop(); return false; }
      this.voices.push({ sound, voice });
      void voice.finished?.then(() => { this.voices = this.voices.filter(item => item.voice !== voice); });
      return true;
    } catch { this.blocked = true; return false; }
    finally { this.pending--; }
  }
  close(): void { if(this.closed)return;this.closed = true; this.stop(); this.driver?.close(); }
}

/** Browser playback is opt-in after a real user gesture; capability is separate from availability. */
export function createBrowserAudioDriver(): AudioDriver | undefined {
  const Constructor = globalThis.AudioContext;
  if (!Constructor) return undefined;
  let context: AudioContext | undefined;
  const voices = new Set<AudioBufferSourceNode>();
  return {
    async resume() { context ??= new Constructor(); await context.resume(); if (context.state !== 'running') throw new Error('Browser audio is locked.'); },
    async play(bytes, volume) {
      if (!context || context.state !== 'running') throw new Error('Browser audio is locked.');
      // Preserve the admitted sample rate and exact PCM length. decodeAudioData
      // resamples to the hardware context rate and can multiply working memory.
      const format=validatePcmWave(bytes),view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
      let cursor=12,dataOffset=0,dataBytes=0;
      while(cursor+8<=bytes.length){const length=view.getUint32(cursor+4,true);if(String.fromCharCode(...bytes.subarray(cursor,cursor+4))==='data'){dataOffset=cursor+8;dataBytes=length;break;}cursor+=8+length+(length%2);}
      const count=dataBytes/(format.channels*2),buffer=context.createBuffer(format.channels,count,format.sampleRate);
      for(let channel=0;channel<format.channels;channel++){const samples=buffer.getChannelData(channel);for(let i=0;i<count;i++)samples[i]=view.getInt16(dataOffset+(i*format.channels+channel)*2,true)/32768;}
      const source = context.createBufferSource(), gain = context.createGain();
      source.buffer = buffer; gain.gain.value = volume; source.connect(gain); gain.connect(context.destination);
      let finished!: () => void;
      const completion = new Promise<void>(resolve => { finished = resolve; });
      source.onended = () => { voices.delete(source); source.disconnect(); gain.disconnect(); finished(); };
      voices.add(source); source.start();
      return { finished: completion, stop() { try { source.stop(); } catch { /* An already ended voice is harmless. */ } } };
    },
    close() { for (const voice of voices) { try { voice.stop(); } catch { /* Already ended. */ } } voices.clear();const closing=context;context=undefined;if(closing&&closing.state!=='closed')void closing.close().catch(()=>undefined); },
  };
}
