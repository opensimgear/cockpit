<script lang="ts">
  import Icon from '../Icon.svelte';

  let {direction, query, change}: {
    direction: 'input' | 'output';
    query: string;
    change: (query: string) => void;
  } = $props();
  let input: HTMLInputElement;
  const label = $derived(direction === 'input' ? 'inputs' : 'outputs');

  function clear() {
    change('');
    input.focus();
  }
</script>

<div class="quick-filter">
  <Icon name="search" size={14}/>
  <input bind:this={input} aria-label={`Filter ${label}`} placeholder={`Filter ${label}…`}
    value={query} oninput={event => change(event.currentTarget.value)}/>
  {#if query}
    <button type="button" aria-label={`Clear ${direction} filter`} onclick={clear}><Icon name="close" size={13}/></button>
  {/if}
</div>

<style>
  .quick-filter{display:flex;align-items:center;gap:6px;min-width:0;min-height:28px;padding:0 7px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg);color:var(--muted)}
  .quick-filter:focus-within{border-color:var(--accent)}
  input{min-width:0;width:100%;height:28px;box-sizing:border-box;padding:0;border:0;outline:0;background:transparent;color:var(--text);font:inherit;font-size:var(--ui-text-size)}
  input::placeholder{color:var(--muted)}
  button{display:grid;place-items:center;flex:0 0 22px;width:22px;height:24px;padding:0;border:0;background:transparent;color:var(--muted);cursor:pointer}
  button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
</style>
