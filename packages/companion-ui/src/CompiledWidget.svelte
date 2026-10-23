<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { createCompiledPixelDecoder, type CompiledWidgetBundle, type CompiledHitRegion } from '../../widget-runtime/src/artifact.ts';
  import {prepareCompiledSurface,type PreparedCompiledSurface} from './compiled-surface.ts';
  import { CompiledWidgetPlayer, CompiledSoundPlayer, createBrowserAudioDriver, type AudioAvailability, type PlayerUpdate } from '../../widget-runtime/src/player.ts';
  import type { BehaviorEvent, BehaviorFeedback, BehaviorState, Scalar } from '../../widget-runtime/src/behavior.ts';
  let { bundle, prepared, name = 'Widget', motion = true, muted = false, interactive = true, showAudioControls = false, onoutput, onstate, onerror, onready, onaudio }: {
    bundle: CompiledWidgetBundle; name?: string; motion?: boolean; muted?: boolean; interactive?: boolean; showAudioControls?: boolean;
    prepared?:PreparedCompiledSurface;
    onoutput?: (output: { port: string; value: Scalar }) => void;
    onstate?: (state: BehaviorState) => void; onerror?: (error: string) => void; onready?: () => void; onaudio?: (availability: AudioAvailability) => void;
  } = $props();
  let canvas: HTMLCanvasElement,canvasHost:HTMLDivElement;
  let surface:PreparedCompiledSurface|undefined,ownsSurface=false;
  let player: CompiledWidgetPlayer | undefined, audio: CompiledSoundPlayer | undefined;
  let pixels: ReturnType<typeof createCompiledPixelDecoder> | undefined;
  let mounted = $state(false), ready = $state(false), suspended=$state(false), error = $state(''), frameId = $state(''), reducedMotion = $state(false);
  let audioAvailability = $state<AudioAvailability>('locked');
  let generation = 0, drawn = '', stateSignature = '', raf = 0;
  const pointers = new Map<number, CompiledHitRegion>(), keys = new Set<string>();
  const now = () => Math.floor(performance.now());
  function report(message: string) { error = message;ready=false;audio?.close();onerror?.(message); }
  function paint(update: PlayerUpdate) {
    if (update.error) { report(update.error);return; }
    for (const effect of update.effects) if (effect.type === 'emit') onoutput?.({ port: effect.port, value: effect.value });
    const signature = JSON.stringify({ values: update.state.values, regions: update.state.regions });
    if (signature !== stateSignature) { stateSignature = signature; onstate?.(structuredClone(update.state)); }
    if (update.frameId !== drawn && canvas && player) {
      const context = canvas.getContext('2d');
      if (!context) { report('A pixel canvas is unavailable.'); return; }
      try {
        const rgba = pixels!.decode(update.frameId);
        context.putImageData(new ImageData(rgba, bundle.artifact.width, bundle.artifact.height), 0, 0);
        drawn = frameId = update.frameId;
      } catch (e) { report(e instanceof Error ? e.message : String(e)); }
    }
    for (const effect of update.effects) if (effect.type === 'cue') void audio?.cue(effect.cue, update.state.nowMs).then(() => audioAvailability = audio?.availability ?? 'unsupported');
  }
  function apply(event: BehaviorEvent) {
    if (!player || !ready) return;
    const previousRegions = JSON.stringify(player.state.regions), update = player.dispatch(event);
    if (previousRegions !== JSON.stringify(update.state.regions)) audio?.exitState();
    paint(update);
  }
  export function dispatch(event: string, updates?: Record<string, Scalar>, feedback?: Record<string, BehaviorFeedback>, suppressCues = false): void {
    apply({ type: 'event', event, nowMs: now(), ...(updates ? { updates } : {}), ...(feedback ? { feedback } : {}), suppressCues });
  }
  export function reset(): void { cancelPointers(); apply({ type: 'reset', nowMs: now() }); }
  /** Retain the last finished pixels while removing every productive input/cue path. */
  export function suspend():void {ready=false;suspended=true;pointers.clear();keys.clear();audio?.close();}
  export async function enableAudio(): Promise<void> { await audio?.unlock(); audioAvailability = audio?.availability ?? 'unsupported'; }
  function hit(region: CompiledHitRegion, phase: 'press' | 'release' | 'activate' | 'cancel') { const event = region.events[phase]; if (event) dispatch(event); }
  function press(event: PointerEvent, region: CompiledHitRegion) {
    if (!interactive || !ready || event.button !== 0 || pointers.has(event.pointerId)) return;
    event.preventDefault(); void enableAudio(); pointers.set(event.pointerId, region);
    try { (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId); hit(region,'press'); }
    catch { pointers.delete(event.pointerId); hit(region,'cancel'); }
  }
  function release(event: PointerEvent, cancel = false) {
    const region = pointers.get(event.pointerId); if (!region) return;
    pointers.delete(event.pointerId);
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const inside = event.clientX >= bounds.left && event.clientX <= bounds.right && event.clientY >= bounds.top && event.clientY <= bounds.bottom;
    if (cancel || !inside) hit(region,'cancel');
    else { hit(region,'release'); hit(region,'activate'); }
  }
  function key(event: KeyboardEvent, region: CompiledHitRegion, down: boolean) {
    if (![' ', 'Enter'].includes(event.key) || !interactive || !ready) return;
    event.preventDefault(); const id = `${region.id}:${event.key}`;
    if (down) { if (event.repeat || keys.has(id)) return; keys.add(id); void enableAudio(); hit(region,'press'); }
    else if (keys.delete(id)) { hit(region,'release'); hit(region,'activate'); }
  }
  function cancelPointers() {
    for (const region of pointers.values()) hit(region,'cancel'); pointers.clear();
    if (keys.size) for (const region of bundle.artifact.hitRegions) hit(region,'cancel'); keys.clear();
  }
  function visibility() {
    const visible = document.visibilityState !== 'hidden'; player?.setVisible(visible); audio?.setVisible(visible);
    if (!visible) cancelPointers(); audioAvailability = audio?.availability ?? 'unsupported';
  }
  $effect(() => { const enabled = motion && !reducedMotion; untrack(() => player?.setMotionEnabled(enabled)); });
  $effect(() => { const value = muted; untrack(() => { audio?.setMuted(value); audioAvailability = audio?.availability ?? 'unsupported'; }); });
  $effect(() => { const availability = audioAvailability;onaudio?.(availability); });
  $effect(() => {
    const input = bundle,candidate=prepared;
    if (!mounted) return;
    const ownGeneration = ++generation,controller=new AbortController();
    suspend();error='';stateSignature='';
    void (candidate?Promise.resolve(candidate):prepareCompiledSurface(input,{signal:controller.signal,nowMs:now()})).then(next => {
      if (ownGeneration !== generation) {if(!candidate)next.dispose();return;}
      if(next.artifactHash!==input.artifactHash)throw new Error('Prepared pixels belong to a different compiled artifact.');
      if(ownsSurface)surface?.dispose();surface=next;ownsSurface=!candidate;canvas=next.canvas;canvas.setAttribute('aria-label',`${name} compiled pixels`);canvasHost.replaceChildren(canvas);
      player=next.player;pixels=next.pixels;drawn=frameId=next.frameId;player.setMotionEnabled(untrack(()=>motion&&!reducedMotion));
      audio = new CompiledSoundPlayer(input, input.artifact.sounds.length ? createBrowserAudioDriver() : undefined); audio.setMuted(untrack(() => muted));
      suspended=false;ready = true; visibility(); paint(player.sample(now()));if(ready)onready?.();
    }).catch(e => { if (ownGeneration === generation) report(e instanceof Error ? e.message : String(e)); });
    return () => {generation++;controller.abort();suspend();};
  });
  onMount(() => {
    mounted = true;
    const preference = window.matchMedia('(prefers-reduced-motion: reduce)'), updatePreference = () => reducedMotion = preference.matches;
    updatePreference();preference.addEventListener('change',updatePreference);
    const tick = () => { if (ready && player && document.visibilityState !== 'hidden') apply({type:'tick',nowMs:now()}); raf = requestAnimationFrame(tick); };
    raf = requestAnimationFrame(tick); document.addEventListener('visibilitychange', visibility); window.addEventListener('blur', cancelPointers);
    return () => { generation++; cancelAnimationFrame(raf); suspend();if(ownsSurface)surface?.dispose();preference.removeEventListener('change',updatePreference); document.removeEventListener('visibilitychange', visibility); window.removeEventListener('blur', cancelPointers); };
  });
</script>

<div class="compiled-widget" data-artifact-id={bundle.artifact.id} data-artifact-hash={bundle.artifactHash} data-frame-id={frameId} aria-label={name}>
  <div class="pixel-surface" bind:this={canvasHost}></div>
  {#if ready && interactive}{#each bundle.artifact.hitRegions as region (region.id)}<button class="compiled-hit" aria-label={region.id === 'hit' ? name : `${name}: ${region.id}`} style:left={`${region.x / bundle.artifact.width * 100}%`} style:top={`${region.y / bundle.artifact.height * 100}%`} style:width={`${region.width / bundle.artifact.width * 100}%`} style:height={`${region.height / bundle.artifact.height * 100}%`} onpointerdown={e=>press(e,region)} onpointerup={e=>release(e)} onpointercancel={e=>release(e,true)} onlostpointercapture={e=>release(e,true)} onkeydown={e=>key(e,region,true)} onkeyup={e=>key(e,region,false)} onblur={cancelPointers}><span class="sr-only">{name}</span></button>{/each}{/if}
  {#if !ready && !error && !suspended}<span class="loading" role="status">Loading compiled visuals…</span>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>
{#if showAudioControls && bundle.artifact.sounds.length}<div class="audio-controls"><button onclick={()=>void enableAudio()} disabled={audioAvailability==='available' || muted} aria-label="Enable sound">♪ Enable sound</button><span role="status">Audio: {audioAvailability}</span></div>{/if}

<style>
  .compiled-widget{position:relative;width:100%;height:100%;min-width:0;min-height:0;isolation:isolate}.pixel-surface{width:100%;height:100%}.pixel-surface :global(canvas){display:block;width:100%;height:100%;object-fit:fill}.compiled-hit{position:absolute;display:block;padding:0;background:transparent;border:0;border-radius:0;touch-action:none;cursor:pointer}.compiled-hit:focus-visible{outline:2px solid #8be2c7;outline-offset:2px}.loading{position:absolute;inset:0;display:grid;place-items:center;font:12px sans-serif;color:#b1c7ce}.error{position:absolute;inset:0;margin:0;display:grid;place-items:center;background:#371e25;color:#ffd1c3;font:12px sans-serif;padding:8px}.audio-controls{display:flex;align-items:center;gap:10px;color:#b4cbd0;font-size:11px}.audio-controls button{font:inherit;padding:6px;border:1px solid #3b5e66;border-radius:4px;background:#213b43;color:inherit;cursor:pointer}.sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}
</style>
