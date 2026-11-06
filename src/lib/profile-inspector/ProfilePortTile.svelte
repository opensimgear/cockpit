<script lang="ts">
  import type { Snippet } from 'svelte';

  let { name, type, title, id, portKey, unavailable = false, disabled = false, action, children }: {
    name: string;
    type: string;
    title: string;
    id?: string;
    portKey?: string;
    unavailable?: boolean;
    disabled?: boolean;
    action?: () => void;
    children?: Snippet;
  } = $props();
</script>

{#snippet content()}
  <strong>{name}</strong><span class="type">{type}</span>
  {@render children?.()}
{/snippet}
{#if action}
  <button type="button" class="port-tile" class:unavailable {title} data-port-id={id} data-port-key={portKey}
    {disabled} aria-label={`Connect ${name}`} onclick={action}>{@render content()}</button>
{:else}
  <div class="port-tile" class:unavailable {title} data-port-id={id} data-port-key={portKey}>
    {@render content()}
  </div>
{/if}

<style>
  .port-tile{display:flex;align-items:center;gap:7px;box-sizing:border-box;width:100%;min-width:0;min-height:var(--ui-input-cell-height);padding:5px 8px;border:1px solid var(--line);border-radius:4px;background:var(--input-bg);color:var(--soft);text-align:left;font-size:var(--ui-text-size)}
  strong{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:570}
  .type{max-width:40%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--muted);font-size:var(--ui-action-size)}
  .unavailable{border-style:dashed}
  .port-tile > :global(svg){flex-shrink:0;color:var(--muted)}
  .port-tile:enabled{cursor:pointer}
  .port-tile:enabled:hover{border-color:var(--accent)}
  .port-tile:disabled{cursor:default;opacity:.5}
  .port-tile:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
</style>
