<script lang="ts">
  import Notification from './Notification.svelte';
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';

  let { only }: { only?: 'vjoy' | 'hidhide' } = $props();
  type Dependency = { id: string; name: string; version: string; purpose: string };
  let dependencies = $state<Dependency[]>([]);
  let supported = $state(false);
  let busy = $state('');
  let message = $state('');
  let error = $state('');

  onMount(() => {
    let disposed = false;
    if (isTauri()) {
      void invoke<{ supported: boolean; dependencies: Dependency[] }>('optional_dependencies')
        .then(result => { if (!disposed) { supported = result.supported; dependencies = result.dependencies; } })
        .catch(e => { if (!disposed) error = String(e); });
    }
    return () => { disposed = true; };
  });

  async function install(dependency: Dependency) {
    busy = dependency.id; message = ''; error = '';
    try { message = await invoke<string>('optional_dependency_install', { id: dependency.id }); }
    catch (e) { error = String(e); }
    finally { busy = ''; }
  }
</script>

{#if supported || error}
  <div class="optional-dependencies">
    <h3>Optional {only === 'vjoy' ? 'vJoy setup' : only === 'hidhide' ? 'HidHide setup' : 'Windows dependencies'}</h3>
    <p>Install only what you need. Setup opens the bundled upstream installer and asks for Windows administrator approval. Close games and controller tools first; Windows may need a restart.</p>
    {#each dependencies.filter(dependency => !only || dependency.id === only) as dependency (dependency.id)}
      <div class="dependency">
        <div><strong>{dependency.name} {dependency.version}</strong><p>{dependency.purpose}</p></div>
        <button class="secondary-button" disabled={!!busy} onclick={() => install(dependency)}>{busy === dependency.id ? 'Waiting for setup…' : `Set up ${dependency.name}`}</button>
      </div>
    {/each}
    {#if message}<Notification message={String(message)} />{/if}
    {#if error}<Notification message={String(error)} kind="error" />{/if}
  </div>
{/if}

<style>
  .optional-dependencies { margin: 16px 0; padding: 14px; border: 1px solid var(--line); border-radius: 6px; background: var(--surface); }
  h3 { font-size: 11px; font-weight: 570; }
  strong { font-size: 10px; font-weight: 570; color: var(--soft); }
  .dependency { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-top: 16px; flex-wrap: wrap; }
  .dependency > div { flex: 1; min-width: min(100%, 200px); }
  p { margin: 8px 0; font-size: 9px; line-height: 1.7; color: var(--muted); overflow-wrap: anywhere; }
</style>
