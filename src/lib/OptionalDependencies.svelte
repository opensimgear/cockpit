<script lang="ts">
  import Notification from './Notification.svelte';
  import Icon from './Icon.svelte';
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
  <details class="panel optional-dependencies">
    <summary><span class="installer-icon"><Icon name="download" size={20} /></span><span class="installer-title"><strong>{only === 'vjoy' ? 'vJoy installer' : only === 'hidhide' ? 'HidHide installer' : 'Driver installers'}</strong><span>Install or repair {only === 'vjoy' ? 'vJoy' : only === 'hidhide' ? 'HidHide' : 'vJoy and HidHide'} on this computer.</span></span>{#if error}<span class="installer-error">Needs attention</span>{/if}<span class="expand-icon"><Icon name="chevron" size={16} /></span></summary>
    <div class="installer-body">
    <p>Install only what you need. Setup opens the bundled upstream installer and asks for Windows administrator approval. Close games and controller tools first; Windows may need a restart.</p>
    {#each dependencies.filter(dependency => !only || dependency.id === only) as dependency (dependency.id)}
      <div class="dependency" role="group" aria-label={dependency.name}>
        <div><strong>{dependency.name} {dependency.version}</strong><p>{dependency.purpose}</p></div>
        <button class="secondary-button" disabled={!!busy} onclick={() => install(dependency)}><Icon name="download" size={14} />{busy === dependency.id ? 'Waiting for setup…' : 'Set up'}</button>
      </div>
    {/each}
    {#if message}<Notification message={String(message)} />{/if}
    {#if error}<Notification message={String(error)} kind="error" />{/if}
    </div>
  </details>
{/if}

<style>
  summary { display: flex; align-items: center; gap: 12px; padding: 20px 22px; cursor: pointer; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  summary:focus-visible { outline: 2px solid var(--accent); outline-offset: -3px; border-radius: 6px; }
  .installer-icon { display: grid; place-items: center; flex-shrink: 0; width: 36px; height: 36px; border: 1px solid var(--line); border-radius: 8px; color: var(--muted); background: var(--input-bg); }
  .installer-title { display: grid; gap: 5px; flex: 1; min-width: 0; }
  .installer-title strong { font-size: 12px; color: var(--text); }
  .installer-title > span { font-size: 9px; color: var(--muted); line-height: 1.7; }
  .installer-error { color: #d3a078; font-size: 9px; }
  .expand-icon { color: var(--muted); flex-shrink: 0; }
  details[open] .expand-icon { transform: rotate(90deg); }
  .installer-body { padding: 0 22px 20px; border-top: 1px solid var(--line); }
  strong { font-size: 10px; font-weight: 570; color: var(--soft); }
  .dependency { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 16px 0; border-top: 1px solid var(--line); flex-wrap: wrap; }
  .dependency > div { flex: 1; min-width: min(100%, 200px); }
  p { margin: 8px 0; font-size: 9px; line-height: 1.7; color: var(--muted); overflow-wrap: anywhere; }
  .installer-body > p { margin: 16px 0; }
  @media(max-width:520px) { summary { padding: 18px 16px; } .installer-body { padding: 0 16px 18px; } .installer-icon { width: 30px; height: 30px; } .dependency > button { margin-left: auto; } .installer-error { display: none; } }
</style>
