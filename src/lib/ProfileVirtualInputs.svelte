<script lang="ts">
  import { onMount } from 'svelte';
  import SystemControllerInputs from './SystemControllerInputs.svelte';
  import { observeProfileRuntime } from './profile-read-models/shared';
  import type { ModelState, RuntimeModel } from './profile-read-models/models';
  import type { NativeProfileOwner } from './profile-runtime';
  import { profileOwnersEqual } from './profile-inspection';
  import { observeProfileNodeSamples, type ProfileNodeSamplePort } from './profile-samples/shared';
  import type { ControllerControl } from './controllers';
  import type { DeepReadonly, ProfileVirtualOutput } from './profiles';
  let { output, profileId = '', revision = 0, active = true }: {
    output: DeepReadonly<ProfileVirtualOutput>; profileId?: string; revision?: number; active?: boolean;
  } = $props();
  let mounted = $state(false), mode = $state<'preview' | 'live' | 'unavailable'>('preview');
  let samples = $state.raw<readonly DeepReadonly<ProfileNodeSamplePort>[]>([]);
  const nodeId = $derived(output.id);
  function inputControls(): ControllerControl[] {
    const values = new Map(samples.filter(port => port.direction === 'input').map(port => [port.endpoint.portId, port.sample]));
    function control(id: string, kind: 'button' | 'axis' | 'hat', index: number, min: number, max: number, fallback: number): ControllerControl {
      const sample = values.get(id);
      const scalar = sample?.status === 'known' && sample.value.kind === 'scalar' ? sample.value.value : undefined;
      const native = mode === 'preview' ? fallback : typeof scalar === 'boolean' ? Number(scalar) : typeof scalar === 'number' ? scalar : null;
      return { capability: { id, kind, usagePage: kind === 'button' ? 9 : 1, usage: kind === 'button' ? index + 1 : kind === 'hat' ? 57 : 48 + index,
        reportId: 0, collection: 0, dataIndex: index, bits: 16, min, max, hasNull: kind === 'hat' },
        value: native === null ? null : { native, normalized: kind === 'hat' && native < 0 ? null : native } };
    }
    return [...output.layout.axes.map((axis, index) => control(axis.portId, 'axis', index, axis.range === 'bipolar' ? -32768 : 0, 32767, axis.fallback)),
      ...output.layout.hats.map((id, index) => control(id, 'hat', index, -1, 7, -1)),
      ...output.layout.buttons.map((id, index) => control(id, 'button', index, 0, 1, 0))];
  }
  const controls = $derived(inputControls());
  onMount(() => { mounted = true; });
  function watchInputs() {
    const id = nodeId, savedId = profileId, savedRevision = revision;
    samples = []; mode = 'preview';
    if (!mounted || !active || !savedId || !savedRevision) return;
    let disposed = false, stopRuntime: (() => void) | undefined, stopSamples: (() => void) | undefined;
    let sampleOwner: DeepReadonly<NativeProfileOwner> | undefined, knownAppliedOwner = false;
    const current = () => !disposed && !document.hidden;
    const clearSamples = () => { stopSamples?.(); stopSamples = undefined; sampleOwner = undefined; samples = []; };
    function runtimeChanged(state: ModelState<RuntimeModel>) {
      if (!current()) return;
      if (state.status !== 'ready' || !state.value) {
        clearSamples(); mode = knownAppliedOwner ? 'unavailable' : 'preview'; return;
      }
      const status = state.value.profileSession, owner = status.owner, applied = status.appliedSnapshot;
      if (!owner || !applied || owner.profileId !== savedId || owner.profileRevision !== savedRevision
        || applied.profileId !== savedId || applied.revision !== savedRevision) {
        clearSamples(); knownAppliedOwner = false; mode = 'preview'; return;
      }
      knownAppliedOwner = true;
      if (sampleOwner && profileOwnersEqual(sampleOwner, owner)) return;
      mode = 'unavailable';
      clearSamples();
      sampleOwner = owner;
      stopSamples = observeProfileNodeSamples(owner, id, next => {
        if (!current() || !sampleOwner || !profileOwnersEqual(sampleOwner, owner)) return;
        if (next.status === 'ready' && next.value && profileOwnersEqual(next.value.owner, owner)) {
          samples = next.value.ports; mode = 'live';
        } else { samples = []; mode = 'unavailable'; }
      });
    }
    function visibility() {
      if (document.hidden) { stopRuntime?.(); stopRuntime = undefined; clearSamples(); knownAppliedOwner = false; mode = 'preview'; }
      else if (!stopRuntime && !disposed) { knownAppliedOwner = false; mode = 'preview'; stopRuntime = observeProfileRuntime(runtimeChanged); }
    }
    document.addEventListener('visibilitychange', visibility);
    visibility();
    return () => { disposed = true; document.removeEventListener('visibilitychange', visibility); stopRuntime?.(); clearSamples(); };
  }
  $effect(watchInputs);
</script>

<section class="virtual-inputs" aria-label="Virtual controller inputs">
  <div class="heading"><h4>Inputs</h4><span>{mode === 'live' ? 'Live' : mode === 'preview' ? 'Preview' : 'Unavailable'}</span></div>
  <SystemControllerInputs {controls} online simple/>
</section>

<style>
  .virtual-inputs { margin-top: 18px; padding-top: 16px; border-top: 1px solid var(--line); min-width: 0; }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 12px; }
  h4 { margin: 0; font-size: var(--ui-heading-size); font-weight: 570; color: var(--soft); }
  .heading span { color: var(--muted); font-size: var(--ui-text-size); }
</style>
