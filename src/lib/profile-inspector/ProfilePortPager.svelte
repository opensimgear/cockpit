<script lang="ts">
  import Icon from '../Icon.svelte';

  let { label, page, pages, offset, count, total, change }: {
    label: string;
    page: number;
    pages: number;
    offset: number;
    count: number;
    total: number;
    change: (page: number) => void;
  } = $props();
</script>

{#if pages > 1}
  <div class="paging">
    <span>{offset + 1}–{offset + count} / {total}</span>
    <div>
      <button type="button" class="previous" aria-label={`Previous ${label}`} disabled={page === 0}
        onclick={() => change(page - 1)}><Icon name="chevron" size={14}/></button>
      <button type="button" aria-label={`Next ${label}`} disabled={page + 1 >= pages}
        onclick={() => change(page + 1)}><Icon name="chevron" size={14}/></button>
    </div>
  </div>
{/if}

<style>
  .paging{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:8px;color:var(--muted);font-size:var(--ui-text-size)}
  .paging>div{display:flex;gap:4px}
  button{display:grid;place-items:center;width:26px;height:26px;padding:0;border:1px solid var(--line);border-radius:4px;background:transparent;color:var(--soft);cursor:pointer}
  button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
  button:disabled{cursor:default;opacity:.5}
  .previous :global(svg){transform:rotate(180deg)}
</style>
