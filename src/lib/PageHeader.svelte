<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';

  let { title, subtitle, eyebrow, level = 1, onback, backLabel = 'Back', backDisabled = false, focusBack = false, titleContent, leading, actions }: {
    title: string;
    subtitle?: string;
    eyebrow?: string;
    level?: 1 | 2;
    onback?: () => void;
    backLabel?: string;
    backDisabled?: boolean;
    focusBack?: boolean;
    titleContent?: Snippet;
    leading?: Snippet;
    actions?: Snippet;
  } = $props();

  function focus(node: HTMLButtonElement) { if (focusBack) node.focus(); }
</script>

<header class="page-header" class:section-header={level === 2}>
  {#if onback}<button class="header-back" onclick={onback} disabled={backDisabled} use:focus><Icon name="back" size={16}/>{backLabel}</button>{/if}
  <div class="header-row">
    <div class="header-title">
      {#if leading}<div class="header-leading">{@render leading()}</div>{/if}
      <div class="header-copy">
        {#if eyebrow}<div class="header-eyebrow">{eyebrow}</div>{/if}
        <svelte:element this={level === 1 ? 'h1' : 'h2'} class="header-heading" aria-label={titleContent ? title : undefined}>
          {#if titleContent}{@render titleContent()}{:else}{title}{/if}
        </svelte:element>
        {#if subtitle}<p class="header-subtitle">{subtitle}</p>{/if}
      </div>
    </div>
    {#if actions}<div class="header-actions">{@render actions()}</div>{/if}
  </div>
</header>

<style>
  .page-header{min-width:0;margin-bottom:var(--ui-section-gap);color:var(--text)}
  .header-row{display:flex;align-items:center;justify-content:space-between;gap:20px;flex-wrap:wrap}
  .header-title{display:flex;align-items:center;gap:16px;min-width:0;flex:1 1 240px}
  .header-copy{min-width:0;flex:1}
  .header-leading{display:flex;align-items:center;flex-shrink:0}
  .header-eyebrow{color:var(--muted);font-weight:600;font-size:8px;letter-spacing:1.7px;text-transform:uppercase;margin-bottom:8px}
  .header-heading{font-size:var(--ui-title-size);font-weight:600;letter-spacing:-.5px;line-height:1.25;margin:0;overflow-wrap:anywhere}
  .section-header .header-heading{font-size:var(--ui-section-title-size)}
  .header-subtitle{font-size:var(--ui-text-size);line-height:var(--ui-line-height);color:var(--muted);margin:8px 0 0}
  .header-actions{display:flex;align-items:center;justify-content:flex-end;gap:8px;flex-wrap:wrap;max-width:100%;min-width:0;margin-left:auto}
  .header-actions :global(button:not(.icon-button):not(.text-button)){min-height:var(--ui-control-height)}
  .header-back{display:inline-flex;align-items:center;gap:6px;padding:6px 0;margin:0 0 16px;border:0;border-radius:4px;background:transparent;color:var(--accent);font:inherit;font-size:var(--ui-action-size);cursor:pointer}
  .header-back:hover:enabled{text-decoration:underline}
  .header-back:disabled{opacity:.45;cursor:default}
  .header-back:focus-visible{outline:2px solid var(--accent);outline-offset:3px}
  @media(max-width:700px){.header-row{align-items:flex-start}.header-actions{flex-wrap:wrap}.header-heading{font-size:24px}}
</style>
