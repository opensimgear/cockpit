<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import { pickGameExecutable } from './gateway';
  import { customGameError, type Game, type GameCategories, type GameRequest, type GamesState } from './games';
  let { game, categories={}, request, close, saved }: { game?: Game; categories?:GameCategories; request: (value: GameRequest) => Promise<GamesState | false>; close: () => void; saved: (value: GamesState) => void } = $props();
  const editingGame = untrack(() => game);
  let name = $state(editingGame?.name ?? ''), executable = $state(editingGame?.executable ?? editingGame?.manualPath ?? '');
  let nameEdited = $state(!!editingGame);
  let categoryId = $state(editingGame?.categoryId ?? '');
  let pending = $state(false), error = $state('');
  const validation = $derived(customGameError(name, executable));
  function show(node: HTMLDialogElement) { node.showModal(); return () => node.close(); }
  function inferName(path: string) { if (!nameEdited) name = path.split(/[\\/]/).at(-1)?.replace(/\.exe$/i, '').replaceAll('_', ' ').slice(0, 80) ?? ''; }
  async function browse() {
    pending = true; error = '';
    try { const path = await pickGameExecutable(editingGame?.id ?? '', executable); if (path) { executable = path; inferName(path); } }
    catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { pending = false; }
  }
  async function save() {
    if (pending || validation) return;
    pending = true; error = '';
    try {
      const result = await request({ command: 'game_custom_save', ...(editingGame ? { gameId: editingGame.id } : {}), name: name.trim(), executable: executable.trim() || null, categoryId:categoryId||null });
      if (!result || result.error) throw new Error(result && result.error || 'The game was not saved. Try again.');
      saved(result);
    } catch (cause) { error = cause instanceof Error ? cause.message : String(cause); }
    finally { pending = false; }
  }
</script>

<dialog class="game-dialog" aria-labelledby="custom-game-title" oncancel={event => { event.preventDefault(); if (!pending) close(); }} {@attach show}>
  <h2 id="custom-game-title"><Icon name="gamepad" size={20}/>{editingGame ? 'Edit game' : 'Add custom game'}</h2>
  <p>Add any game to use its own bindings and activate matching profiles.</p>
  <form onsubmit={event => { event.preventDefault(); void save(); }}>
    <label>Game executable<div class="path-row"><input aria-label="Custom game executable" bind:value={executable} oninput={event=>inferName(event.currentTarget.value)} placeholder="C:\Games\My game\game.exe" disabled={pending}/><button type="button" class="secondary-button" onclick={browse} disabled={pending}><Icon name="folder" size={14}/>Browse</button></div></label>
    <p class="help">Choose the game process used for bindings. You can add it without a path and choose the executable later.</p>
    <label>Game name<input aria-label="Custom game name" bind:value={name} oninput={()=>nameEdited=true} maxlength="80" disabled={pending} placeholder="My game"/></label>
    <label>Category<select aria-label="Custom game category" bind:value={categoryId} disabled={pending}><option value="">Uncategorized</option>{#each Object.entries(categories) as [id,title] (id)}<option value={id}>{title}</option>{/each}</select></label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions"><button type="button" class="secondary-button" onclick={close} disabled={pending}>Cancel</button><button class="primary-button" title={validation||undefined} disabled={pending||!!validation}>{pending ? 'Saving…' : editingGame ? 'Save game' : 'Add game'}</button></div>
  </form>
</dialog>

<style>
  .game-dialog{position:fixed;inset:0;margin:auto;width:min(560px,calc(100% - 32px));max-height:calc(100% - 40px);overflow:auto;box-sizing:border-box;padding:24px;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}.game-dialog::backdrop{background:#0009}h2{display:flex;align-items:center;gap:10px;font-size:18px;margin:0}p{color:var(--muted);font-size:12px;line-height:1.6;margin:16px 0}form{display:grid;gap:16px}label{display:grid;gap:8px;font-size:12px;color:var(--soft)}input{min-width:0;width:100%;box-sizing:border-box;padding:10px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--text);font-size:12px}.path-row{display:flex;gap:8px}.path-row input{flex:1}.path-row button{flex-shrink:0;display:flex;gap:6px;align-items:center}.help{font-size:11px;margin:-8px 0 0}.actions{display:flex;justify-content:flex-end;gap:10px;margin-top:8px}.error{color:#e5b19b;margin:0}button{font-size:12px}
</style>
