<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { pickGameExecutable } from './gateway';
  import type { GameLibraryCandidate, GameRequest, GamesState } from './games';
  let { request, close, imported }: { request: (value: GameRequest) => Promise<GamesState | false>; close: () => void; imported: (value: GamesState) => void } = $props();
  let candidates = $state<GameLibraryCandidate[]>([]), diagnostics = $state<string[]>([]), search = $state(''), showTools = $state(false);
  let selections = $state<Record<string, boolean>>({}), executables = $state<Record<string, string>>({});
  let pending = $state(false), scanned = $state(false), error = $state(''), progress = $state('');
  const visible = $derived(candidates.filter(candidate => (showTools || candidate.kind !== 'tool') && `${candidate.name} ${candidate.client}`.toLowerCase().includes(search.toLowerCase())));
  const selected = $derived(candidates.filter(candidate => selections[candidate.id]));
  function show(node: HTMLDialogElement) { node.showModal(); return () => node.close(); }
  async function scan() {
    if (pending) return;
    pending = true; error = ''; progress = 'Scanning installed game libraries…';
    try {
      const result = await request({ command: 'game_library_scan' });
      if (!result || result.error) throw new Error(result && result.error || 'Game libraries could not be scanned.');
      candidates = result.libraryCandidates ?? []; diagnostics = result.libraryDiagnostics ?? []; scanned = true;
      selections = {}; executables = Object.fromEntries(candidates.map(candidate => [candidate.id, candidate.executableCandidates.length === 1 ? candidate.executableCandidates[0] : '']));
    } catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { pending = false; progress = ''; }
  }
  onMount(() => { void scan(); });
  async function browse(candidate: GameLibraryCandidate) {
    pending = true; error = '';
    try { const path = await pickGameExecutable(candidate.gameId ?? '', executables[candidate.id]); if (path) executables = { ...executables, [candidate.id]: path }; }
    catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { pending = false; }
  }
  async function importSelected() {
    if (pending || !selected.length) return;
    pending = true; error = '';
    const batch = [...selected]; let last: GamesState | undefined;
    try {
      for (let index = 0; index < batch.length; index++) {
        const candidate = batch[index]; progress = `Adding ${index + 1} of ${batch.length}: ${candidate.name}`;
        const result = await request({ command: 'game_library_import', candidateId: candidate.id, executable: executables[candidate.id]?.trim() || null });
        if (!result || result.error) throw new Error(result && result.error || `${candidate.name} was not added.`);
        last = result; selections = { ...selections, [candidate.id]: false };
      }
      if (last) imported(last);
    } catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { pending = false; progress = ''; }
  }
</script>

<dialog class="library-dialog" aria-labelledby="library-game-title" oncancel={event=>{event.preventDefault();if(!pending)close();}} {@attach show}>
  <div class="library-heading"><h2 id="library-game-title"><Icon name="gamepad" size={20}/>Import from game library</h2><button class="secondary-button" disabled={pending} onclick={scan}><Icon name="refresh" size={14}/>Rescan</button></div>
  <p>Choose installed games from your local clients. Review the executable used for scoped bindings and profile activation. Games with integrations use their existing entries.</p>
  <div class="library-filters"><input type="search" aria-label="Search game library" bind:value={search} placeholder="Search games or clients" disabled={pending}/><label><input type="checkbox" bind:checked={showTools} disabled={pending}/>Show tools</label></div>
  {#if diagnostics.length}<details class="diagnostics"><summary>Library scan details</summary><ul>{#each diagnostics as diagnostic,index (index)}<li>{diagnostic}</li>{/each}</ul></details>{/if}
  <ul class="library-candidates" aria-label="Discovered library games">
    {#each visible as candidate (candidate.id)}
      <li class:selected={!!selections[candidate.id]}>
        <label class="candidate-choice"><input type="checkbox" aria-label={`Import ${candidate.name}`} checked={!!selections[candidate.id]} onchange={event=>selections={...selections,[candidate.id]:event.currentTarget.checked}} disabled={pending}/><span><strong>{candidate.name}</strong><small>{candidate.client} · {candidate.gameId?'Integrated game':candidate.kind==='tool'?'Tool':candidate.kind==='unknown'?'Review game':'Custom game'}</small><small class="install-path">{candidate.installPath}</small></span></label>
        {#if selections[candidate.id]}
          <div class="candidate-executable">
            {#if candidate.executableCandidates.length>1}<label>Detected executables<select aria-label={`Detected executables for ${candidate.name}`} value={executables[candidate.id]} onchange={event=>executables={...executables,[candidate.id]:event.currentTarget.value}} disabled={pending}><option value="">Choose the game executable</option>{#each candidate.executableCandidates as executable (executable)}<option value={executable}>{executable}</option>{/each}</select></label>{/if}
            <label>Game executable<div class="path-row"><input aria-label={`Executable for ${candidate.name}`} value={executables[candidate.id]??''} oninput={event=>executables={...executables,[candidate.id]:event.currentTarget.value}} placeholder="Choose later, or enter the full executable path" disabled={pending}/><button class="secondary-button" onclick={()=>browse(candidate)} disabled={pending}><Icon name="folder" size={14}/>Browse</button></div></label>
            {#if !executables[candidate.id]}<p class="candidate-help">Add now and choose its executable later. Scoped bindings and activation will wait for an executable.</p>{/if}
          </div>
        {/if}
      </li>
    {:else}<li class="library-empty">{!scanned?'Your installed games will appear here.':search?'No games match this search.':'No installed games found. Add a game manually by choosing its executable.'}</li>{/each}
  </ul>
  {#if progress}<p class="library-progress" role="status">{progress}</p>{/if}
  {#if error}<p class="library-error" role="alert">{error}</p>{/if}
  <div class="library-actions"><span>{selected.length} selected</span><button class="secondary-button" onclick={close} disabled={pending}>Cancel</button><button class="primary-button" disabled={pending||!selected.length} onclick={importSelected}>{pending?'Working…':`Add selected games${selected.length?` (${selected.length})`:''}`}</button></div>
</dialog>

<style>
  .library-dialog{position:fixed;inset:0;margin:auto;width:min(820px,calc(100% - 32px));max-height:calc(100% - 40px);overflow:auto;box-sizing:border-box;padding:24px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}.library-dialog::backdrop{background:#0009}.library-heading{display:flex;gap:14px;justify-content:space-between;align-items:center}h2{display:flex;align-items:center;gap:10px;font-size:18px;margin:0}p{font-size:12px;color:var(--muted);line-height:1.6}.library-filters{display:flex;align-items:center;gap:18px;margin:18px 0}.library-filters>input{flex:1;min-width:0}.library-filters label{display:flex;align-items:center;gap:8px;font-size:12px;white-space:nowrap}input:not([type=checkbox]),select{min-width:0;box-sizing:border-box;padding:9px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text);font-size:12px}.library-candidates{display:grid;gap:10px;list-style:none;padding:0;margin:14px 0;max-height:50vh;overflow:auto}.library-candidates>li{border:1px solid var(--line);border-radius:6px;padding:12px}.library-candidates>li.selected{border-color:var(--accent)}.candidate-choice{display:flex;align-items:flex-start;gap:12px;cursor:pointer}.candidate-choice>input{margin-top:3px}.candidate-choice>span{display:grid;gap:5px;min-width:0}.candidate-choice strong{font-size:13px}.candidate-choice small{color:var(--muted);font-size:11px}.install-path{overflow-wrap:anywhere}.candidate-executable{display:grid;gap:10px;margin-top:14px;margin-left:25px}.candidate-executable label{display:grid;gap:7px;font-size:11px;color:var(--soft)}.path-row{display:flex;gap:8px;min-width:0}.path-row input{width:100%}.path-row button,.library-heading button{display:flex;align-items:center;gap:6px;flex-shrink:0}.candidate-help{font-size:11px;margin:0}.library-empty{font-size:12px;color:var(--muted);line-height:1.6}.diagnostics{font-size:11px;color:var(--muted);line-height:1.7}.library-progress{color:var(--accent)}.library-error{color:#e5b19b}.library-actions{display:flex;align-items:center;justify-content:flex-end;gap:10px;margin-top:18px}.library-actions>span{margin-right:auto;font-size:12px;color:var(--muted)}button{font-size:12px}@media(max-width:500px){.library-heading{align-items:flex-start}h2{font-size:16px}.library-actions{flex-wrap:wrap}.library-actions>span{width:100%}.candidate-executable{margin-left:0}}
</style>
