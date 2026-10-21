<script lang="ts">
  import DesktopPreferences from './DesktopPreferences.svelte';
  import ProgressNotificationSettings from './ProgressNotificationSettings.svelte';
  import OutputSettings from './OutputSettings.svelte';
  import CompanionListenerSettings from './CompanionListenerSettings.svelte';
  import { canvasStyling, setArrowStyle, setCornerRadius, setCollapseParallelWires, type CanvasArrowStyle } from './canvas-styling';
  import { canvasNavigation, setWheelAction, type CanvasWheelAction } from './canvas-navigation';
  import { diagramPendingPath } from './profile-diagram';

  let { light, onToggleTheme }: { light: boolean; onToggleTheme: () => void } = $props();
  const tabs = [
    { id: 'general', label: 'General' },
    { id: 'styling', label: 'Styling' },
    { id: 'outputs', label: 'Outputs' },
    { id: 'companions', label: 'Companions' },
  ] as const;
  let selected = $state<(typeof tabs)[number]['id']>('general');
  const preview = $derived(diagramPendingPath({ x: 24, y: 30 }, { x: 260, y: 130 }, 'output', $canvasStyling.arrowStyle, $canvasStyling.cornerRadius));

  function tabKeydown(event: KeyboardEvent, index: number) {
    let next: number;
    if (event.key === 'ArrowRight') next = (index + 1) % tabs.length;
    else if (event.key === 'ArrowLeft') next = (index + tabs.length - 1) % tabs.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = tabs.length - 1;
    else return;
    event.preventDefault();
    selected = tabs[next].id;
    const button = event.currentTarget as HTMLButtonElement;
    button.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  }
</script>

<div class="page-heading"><div><div class="eyebrow">MAKE IT YOURS</div><h1>Settings</h1><p>A familiar cockpit, arranged around the way you work.</p></div></div>

<div class="settings-tabs" role="tablist" aria-label="Settings sections">
  {#each tabs as tab, index (tab.id)}
    <button id={`settings-tab-${tab.id}`} role="tab" aria-selected={selected === tab.id} aria-controls={`settings-panel-${tab.id}`} tabindex={selected === tab.id ? 0 : -1} onclick={() => { selected = tab.id; }} onkeydown={event => tabKeydown(event, index)}>{tab.label}</button>
  {/each}
</div>

<div class="settings-content" id="settings-panel-general" role="tabpanel" aria-labelledby="settings-tab-general" tabindex="0" hidden={selected !== 'general'}>
  <DesktopPreferences />
  <section class="panel preferences-panel canvas-settings" aria-label="Canvas navigation">
    <div class="panel-heading"><h2>Canvas</h2></div>
    <div class="preference-row">
      <div><strong>Mouse wheel</strong><p id="canvas-wheel-help">Choose the mouse wheel action on profile canvases. Hold Ctrl/⌘ to reverse the action. Changes are saved automatically.</p></div>
      <label class="style-field"><span class="field-label">Mouse wheel</span><select aria-label="Canvas mouse wheel" aria-describedby="canvas-wheel-help" value={$canvasNavigation.wheelAction} onchange={event => setWheelAction(event.currentTarget.value as CanvasWheelAction)}><option value="pan">Scroll (pan)</option><option value="zoom">Zoom</option></select></label>
    </div>
    {#if $canvasNavigation.storageError}<div class="preference-row"><p class="storage-error" role="alert">{$canvasNavigation.storageError}</p></div>{/if}
  </section>
  <ProgressNotificationSettings />
</div>

<div class="settings-content" id="settings-panel-styling" role="tabpanel" aria-labelledby="settings-tab-styling" tabindex="0" hidden={selected !== 'styling'}>
  <section class="panel preferences-panel">
    <div class="panel-heading"><h2>Appearance</h2></div>
    <div class="preference-row">
      <div><strong>Color theme</strong><p>{light ? 'Light' : 'Dark'} theme. Changes are saved automatically.</p></div>
      <button class="toggle" class:on={light} role="switch" aria-label="Light theme" aria-checked={light} onclick={onToggleTheme}><span></span></button>
    </div>
  </section>
  <section class="panel preferences-panel arrows-settings" aria-label="Arrow styling">
    <div class="panel-heading"><h2>Arrows</h2></div>
    <div class="preference-row">
      <div><strong>Arrow style</strong><p id="arrow-style-help">Choose how connections are drawn on every profile canvas. Changes are saved automatically.</p></div>
      <label class="style-field"><span class="field-label">Arrow style</span><select aria-label="Arrow style" aria-describedby="arrow-style-help" value={$canvasStyling.arrowStyle} onchange={event => setArrowStyle(event.currentTarget.value as CanvasArrowStyle)}><option value="smooth">Fully smooth (Bézier)</option><option value="orthogonal">Horizontal / vertical</option></select></label>
    </div>
    <div class="preference-row">
      <div><strong id="arrow-collapse-label">Collapse parallel connections</strong><p id="arrow-collapse-help">Share trunks at common input or output points, with a signal count and branches to each block.</p></div>
      <input type="checkbox" aria-labelledby="arrow-collapse-label" aria-describedby="arrow-collapse-help" checked={$canvasStyling.collapseParallelWires} onchange={event => setCollapseParallelWires(event.currentTarget.checked)} />
    </div>
    {#if $canvasStyling.arrowStyle === 'orthogonal'}
      <div class="preference-row">
        <div><strong>Corner radius</strong><p id="arrow-corner-radius-help">Round the corners between horizontal and vertical segments. Set to 0 for square corners.</p></div>
        <label class="radius-field"><span class="field-label">Corner radius</span><span class="radius-input"><input class="text-input" aria-label="Arrow corner radius" aria-describedby="arrow-corner-radius-help" type="number" min="0" max="100" step="1" value={$canvasStyling.cornerRadius} oninput={event => { if (event.currentTarget.value !== '') setCornerRadius(event.currentTarget.valueAsNumber); }} onblur={event => { event.currentTarget.value = String($canvasStyling.cornerRadius); }} /><span aria-hidden="true">px</span></span></label>
      </div>
    {/if}
    <div class="arrow-preview" aria-label="Arrow style preview">
      <svg viewBox="0 0 284 160" role="img" aria-label={$canvasStyling.arrowStyle === 'smooth' ? 'Fully smooth connection preview' : 'Horizontal and vertical connection preview'}>
        <path d={preview} class="preview-line" />
        <circle cx="24" cy="30" r="4" />
        <path d="M 253 125 L 260 130 L 253 135" class="preview-head" />
      </svg>
    </div>
    {#if $canvasStyling.storageError}<div class="preference-row"><p class="storage-error" role="alert">{$canvasStyling.storageError}</p></div>{/if}
  </section>
</div>

<div class="settings-content" id="settings-panel-outputs" role="tabpanel" aria-labelledby="settings-tab-outputs" tabindex="0" hidden={selected !== 'outputs'}>
  <OutputSettings />
</div>

<div class="settings-content" id="settings-panel-companions" role="tabpanel" aria-labelledby="settings-tab-companions" tabindex="0" hidden={selected !== 'companions'}>
  <CompanionListenerSettings />
</div>

<style>
  .settings-tabs{display:flex;gap:6px;max-width:900px;border-bottom:1px solid var(--line);margin-bottom:24px;overflow-x:auto}
  .settings-tabs button{padding:12px 18px;border:0;border-bottom:2px solid transparent;background:transparent;color:var(--muted);font-size:11px;white-space:nowrap;cursor:pointer}
  .settings-tabs button:hover{color:var(--text);background:var(--surface-raised)}
  .settings-tabs button[aria-selected="true"]{border-bottom-color:var(--accent);color:var(--text)}
  .settings-tabs button:focus-visible{outline:2px solid var(--accent);outline-offset:-4px;border-radius:4px}
  .settings-content{max-width:900px}
  .settings-content > :global(section:first-child){margin-top:0}
  .settings-content:focus{outline:none}
  .arrows-settings,.canvas-settings{margin-top:24px}
  .preference-row p{max-width:560px;line-height:1.6}
  .style-field,.radius-field{display:grid;gap:8px;flex-shrink:0}
  .style-field{min-width:200px}
  .radius-field{width:124px}
  select{width:100%;background:var(--input-bg)}
  .radius-input{display:flex;align-items:center;gap:8px}
  .radius-input input{width:100%;min-width:0;padding-right:10px}
  .radius-input>span{font-size:10px;color:var(--muted)}
  .arrow-preview{padding:12px 24px;color:var(--accent);background:var(--input-bg)}
  .arrow-preview svg{display:block;width:284px;max-width:100%;height:120px;margin:0 auto;overflow:visible}
  .preview-line,.preview-head{fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}
  .arrow-preview circle{fill:currentColor}
  .preference-row .storage-error{color:var(--text)}
  .arrows-settings input[type="checkbox"]{width:18px;height:18px;flex-shrink:0;accent-color:var(--accent);cursor:pointer}
  @media(max-width:620px){.preference-row{flex-wrap:wrap}.style-field{min-width:min(100%,200px)}.settings-tabs button{padding:12px 13px}}
</style>
