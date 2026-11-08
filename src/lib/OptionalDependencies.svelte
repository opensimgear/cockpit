<script module lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { notify } from './notifications';

  type DependencyId = 'vjoy' | 'hidhide';
  type Dependency = { id: DependencyId; name: string; version: string; purpose: string };
  let active = $state<DependencyId | null>(null);
  let failure = $state.raw<{ id: DependencyId; message: string } | null>(null);

  export function driverSetupActive(id: DependencyId) {
    return active === id;
  }

  async function install(id: DependencyId) {
    if (active) return;
    active = id;
    failure = null;
    try {
      notify(await invoke<string>('optional_dependency_install', { id }));
    } catch (reason) {
      const message = String(reason);
      failure = { id, message };
      notify(message, 'error');
    } finally {
      active = null;
    }
  }
</script>

<script lang="ts">
  import Notification from './Notification.svelte';
  import Icon from './Icon.svelte';
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';

  let { only, disabled = false }: { only: DependencyId; disabled?: boolean } = $props();
  let dependencies = $state<Dependency[]>([]);
  let supported = $state(false), loaded = $state(false), loadError = $state('');
  let disposed = false;
  const dependency = $derived(dependencies.find(item => item.id === only));
  const error = $derived(loadError || (failure?.id === only ? failure.message : ''));

  onMount(() => {
    if (isTauri()) {
      void invoke<{ supported: boolean; dependencies: Dependency[] }>('optional_dependencies')
        .then(result => { if (!disposed) { supported = result.supported; dependencies = result.dependencies; } })
        .catch(reason => { if (!disposed) loadError = String(reason); })
        .finally(() => { if (!disposed) loaded = true; });
    } else loaded = true;
    return () => { disposed = true; };
  });

</script>

<div class="dependency-setup">
  {#if !loaded}<p role="status">Loading setup…</p>
  {:else if supported && dependency}
    <div class="setup-row" role="group" aria-label={`${dependency.name} setup`}>
      <div><strong>Install {dependency.name} {dependency.version}</strong><p>Close games and controller tools first. Windows will ask for administrator approval and may require a restart.</p></div>
      <button class="secondary-button" disabled={disabled || active !== null} onclick={() => install(only)}><Icon name="download" size={14} />{active === only ? 'Waiting for setup…' : 'Set up'}</button>
    </div>
  {:else if !error}<p>Driver setup is available in the Windows desktop app.</p>{/if}
  {#if error}<p class="setup-error" role="alert">{error}</p>{/if}
  {#if loadError}<Notification message={loadError} kind="error" />{/if}
</div>

<style>
  .dependency-setup { width: 100%; min-width: 0; }
  .setup-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
  .setup-row > div { flex: 1; min-width: min(100%, 240px); }
  strong { font-size: var(--ui-heading-size); font-weight: 570; color: var(--soft); }
  p { margin-top: 6px; font-size: var(--ui-text-size); line-height: var(--ui-line-height); color: var(--muted); overflow-wrap: anywhere; }
  .setup-row button { flex-shrink: 0; margin-left: auto; }
  .setup-error { color: #d3a078; }
</style>
