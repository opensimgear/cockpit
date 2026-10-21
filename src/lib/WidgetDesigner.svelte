<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import {
    createWidgetEditorHistory, applyWidgetEditorCommand, undoWidgetEditor, redoWidgetEditor,
    resolveWidgetLayers, WIDGET_STATES, newId,
    type WidgetDefinition, type WidgetLayer, type WidgetState, type WidgetSample, type WidgetProperty,
    type WidgetEditorCommand, type Bounds,
  } from '@opensimgear/companion-designer';
  import { WidgetView, movedBounds, resizedCornerBounds } from '@opensimgear/companion-ui';
  let { definition, assets = {}, assetOptions = [], onsave, oncancel }: {
    definition: WidgetDefinition; assets?: Record<string, string>; assetOptions?: { id: string; label: string }[];
    onsave: (definition: WidgetDefinition) => Promise<void>; oncancel: () => void;
  } = $props();
  let history = $state.raw(untrack(() => createWidgetEditorHistory(definition)));
  let selected = $state(untrack(() => history.present.layers[0].id));
  let previewState = $state<WidgetState | 'base'>('base'), addKind = $state<WidgetLayer['kind']>('rectangle');
  let propertyType = $state<WidgetProperty['type']>('text'), error = $state(''), busy = $state(false), snap = $state(true);
  let dialog: HTMLDialogElement, stage: HTMLDivElement;
  const draft = $derived(history.present);
  const layer = $derived(draft.layers.find(l => l.id === selected));
  const layers = $derived(resolveWidgetLayers(draft, {}, previewState === 'base' ? 'normal' : previewState));
  let drag = $state<{ pointer: number; x: number; y: number; bounds: Bounds; preview: Bounds; resize: boolean } | null>(null);
  const shownBounds = $derived(drag?.preview ?? layer?.bounds);
  const simulated = $derived(Object.fromEntries(draft.outputs.map(port => [port.name, {
    outputId: 'preview-output', channelId: 'preview-channel', receivedAtMs: 0, sourceAgeMs: 0,
    status: ['unknown', 'stale', 'disconnected'].includes(previewState) ? previewState : 'fresh',
    value: port.kind === 'indicator' ? previewState === 'active' : port.kind === 'gauge' ? ((port.range?.min ?? 0) + (port.range?.max ?? 100)) / 2 : 'Sample',
    provenance: 'game-confirmed', sourceKind: 'simulator', unit: port.unit ?? 'ratio', ...(port.range ? { range: port.range } : {}),
  } as WidgetSample])));
  onMount(() => { dialog.showModal(); return () => dialog.close(); });
  function command(command: WidgetEditorCommand) {
    try { history = applyWidgetEditorCommand(history, command); if (!draft.layers.some(l => l.id === selected)) selected = draft.layers[0].id; error = ''; return true; }
    catch (e) { error = e instanceof Error ? e.message : String(e); return false; }
  }
  function patch(patch: Record<string, unknown>) { if (layer) command({ type: 'update-layer', id: layer.id, patch }); }
  function paintValue(key: string): unknown {
    const base = layer as unknown as Record<string, unknown> | undefined;
    return previewState !== 'base' ? (layer?.states?.[previewState] as Record<string, unknown> | undefined)?.[key] ?? base?.[key] : base?.[key];
  }
  function literal(value: unknown, fallback: string | number = ''): string | number | boolean {
    if (value && typeof value === 'object' && 'property' in value) return draft.properties.find(p => p.name === value.property)?.default ?? fallback;
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean' ? value : fallback;
  }
  function binding(value: unknown): string { return value && typeof value === 'object' && 'property' in value ? String(value.property) : ''; }
  function style(key: string, value: unknown) {
    if (!layer) return;
    if (previewState === 'base') patch({ [key]: value });
    else command({ type: 'state-style', id: layer.id, state: previewState, paint: { ...layer.states?.[previewState], [key]: value } });
  }
  function geometry(field: keyof Bounds, value: number) { if (layer) patch({ bounds: { ...layer.bounds, [field]: value / 100 } }); }
  function begin(event: PointerEvent, resize: boolean) {
    if (!layer || event.button !== 0) return; event.preventDefault();
    drag = { pointer: event.pointerId, x: event.clientX, y: event.clientY, bounds: { ...layer.bounds }, preview: { ...layer.bounds }, resize };
    try { (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId); } catch { drag = null; }
  }
  function move(event: PointerEvent) {
    if (!drag || drag.pointer !== event.pointerId) return;
    const rect = stage.getBoundingClientRect(), dx = (event.clientX - drag.x) / rect.width, dy = (event.clientY - drag.y) / rect.height;
    const grid = { columns: 40, rows: 40, snap: snap && !event.ctrlKey };
    drag.preview = drag.resize ? resizedCornerBounds(drag.bounds, dx, dy, grid, 'se') : movedBounds(drag.bounds, dx, dy, grid);
  }
  function finish(event: PointerEvent) { if (!drag || drag.pointer !== event.pointerId) return; const bounds = { ...drag.preview }; drag = null; patch({ bounds }); }
  function addLayer() { if (command({ type: 'add-layer', kind: addKind })) selected = draft.layers.at(-1)!.id; }
  function addProperty() {
    let index = draft.properties.length + 1; while (draft.properties.some(p => p.name === `property-${index}`)) index++;
    const name = `property-${index}`;
    const property: WidgetProperty = propertyType === 'number' ? { name, label: 'Number', type: 'number', default: 0, min: -1000, max: 1000 } : propertyType === 'boolean' ? { name, label: 'Visible', type: 'boolean', default: true } : { name, label: propertyType === 'color' ? 'Color' : propertyType === 'asset' ? 'Artwork' : 'Text', type: propertyType, default: propertyType === 'color' ? '#63dfd0' : '' };
    command({ type: 'set-property', property });
  }
  function updateProperty(property: WidgetProperty, patch: Record<string, unknown>) { command({ type: 'set-property', property: { ...property, ...patch } as WidgetProperty }); }
  function interaction(kind: string) {
    const next = structuredClone(draft);
    if (kind === 'momentary' || kind === 'toggle') { next.inputs = [{ name: next.inputs[0]?.kind === 'button' ? next.inputs[0].name : 'press', label: 'Press', kind: 'button' }]; next.interaction = { kind, port: next.inputs[0].name }; }
    else if (kind === 'selector') { next.inputs = [{ name: 'selection', label: 'Selection', kind: 'axis', axisMode: 'unipolar', fallback: 0 }]; next.interaction = { kind: 'selector', port: 'selection', options: [{ label: 'Off', value: 0 }, { label: 'On', value: 1 }] }; }
    else if (kind === 'navigate') { next.inputs = []; if (!next.properties.some(p => p.name === 'page')) next.properties.push({ name: 'page', label: 'Destination page', type: 'text', default: '' }); next.interaction = { kind: 'navigate', pageProperty: 'page' }; }
    else { next.inputs = []; next.interaction = { kind: 'none' }; }
    command({ type: 'replace', definition: next });
  }
  function addFeedback(kind: 'indicator' | 'text' | 'gauge') {
    command({ type: 'update', patch: { outputs: [...draft.outputs, { name: newId('feedback'), label: kind === 'indicator' ? 'Status' : 'Value', kind, ...(kind === 'gauge' ? { unit: 'ratio', range: { min: 0, max: 1 } } : {}) }] } });
  }
  function statePort(key: 'confirmedPort' | 'warningPort' | 'enabledPort', name: string) { const state = { ...draft.state }; if (name) state[key] = name; else delete state[key]; command({ type: 'update', patch: { state } }); }
  async function save() { busy = true; error = ''; try { await onsave(structuredClone(draft)); } catch (e) { error = e instanceof Error ? e.message : String(e); } finally { busy = false; } }
  function cancel() { if (history.past.length && !window.confirm('Discard unsaved widget design changes?')) return; oncancel(); }
</script>

<dialog class="widget-designer" bind:this={dialog} aria-label="Widget designer" oncancel={event => { event.preventDefault(); cancel(); }}>
  <header class="designer-heading">
    <div><span class="eyebrow">REUSABLE WIDGET</span><h2>Widget designer</h2><p>Design once for browser, Android and ESP32 companions.</p></div>
    <div class="designer-actions"><button aria-label="Undo widget edit" disabled={!history.past.length || busy} onclick={() => history = undoWidgetEditor(history)}>Undo</button><button aria-label="Redo widget edit" disabled={!history.future.length || busy} onclick={() => history = redoWidgetEditor(history)}>Redo</button><button disabled={busy} onclick={cancel}>Cancel</button><button class="primary" disabled={busy} onclick={save}>{busy ? 'Saving…' : 'Save to widget library'}</button></div>
  </header>
  {#if error}<p class="designer-error" role="alert">{error}</p>{/if}
  <div class="designer-workspace">
    <aside class="layers-panel" aria-label="Widget layers">
      <label>Widget name<input aria-label="Widget name" value={draft.title} onchange={e => command({ type: 'update', patch: { title: e.currentTarget.value } })} /></label>
      <h3>Layers <small>back to front</small></h3>
      <ol>{#each draft.layers as item, index (item.id)}<li><button class:selected={selected === item.id} aria-label={`Select layer ${item.label}`} onclick={() => selected = item.id}><span>{index + 1}</span><strong>{item.label}</strong><small>{item.kind}</small></button></li>{/each}</ol>
      <div class="add-layer"><select aria-label="New layer type" bind:value={addKind}>{#each ['rectangle', 'ellipse', 'line', 'text', 'image', 'indicator', 'gauge'] as kind}<option value={kind}>{kind}</option>{/each}</select><button onclick={addLayer}>Add layer</button></div>
      {#if layer}<div class="layer-actions"><button disabled={draft.layers.indexOf(layer) === 0} onclick={() => command({ type: 'reorder-layer', id: layer!.id, index: draft.layers.indexOf(layer!) - 1 })}>Move back</button><button disabled={draft.layers.indexOf(layer) === draft.layers.length - 1} onclick={() => command({ type: 'reorder-layer', id: layer!.id, index: draft.layers.indexOf(layer!) + 1 })}>Move forward</button><button onclick={() => { if (command({ type: 'duplicate-layer', id: layer!.id })) selected = draft.layers.at(-1)!.id; }}>Duplicate layer</button><button disabled={draft.layers.length === 1} onclick={() => command({ type: 'delete-layer', id: layer!.id })}>Delete layer</button></div>{/if}
      <details><summary>Interaction and feedback</summary>
        <label>Interaction<select value={draft.interaction.kind} onchange={e => interaction(e.currentTarget.value)}>{#each ['none', 'momentary', 'toggle', 'selector', 'navigate'] as kind}<option value={kind}>{kind}</option>{/each}</select></label>
        {#each ['confirmedPort', 'warningPort', 'enabledPort'] as key}<label>{key === 'confirmedPort' ? 'Confirmed active' : key === 'warningPort' ? 'Warning' : 'Enabled'}<select value={draft.state[key as keyof typeof draft.state] ?? ''} onchange={e => statePort(key as keyof typeof draft.state, e.currentTarget.value)}><option value="">No feedback binding</option>{#each draft.outputs.filter(p => p.kind === 'indicator') as port}<option value={port.name}>{port.label}</option>{/each}</select></label>{/each}
        {#each draft.outputs as port, index}<div class="port"><label>Port label<input value={port.label} onchange={e => command({ type: 'update', patch: { outputs: draft.outputs.map((p, i) => i === index ? { ...p, label: e.currentTarget.value } : p) } })} /></label><small>{port.kind} · {port.name}</small>{#if port.kind === 'gauge'}<label>Unit<input value={port.unit ?? ''} onchange={e => command({ type: 'update', patch: { outputs: draft.outputs.map((p, i) => i === index ? { ...p, unit: e.currentTarget.value } : p) } })} /></label>{/if}</div>{/each}
        <div class="layer-actions"><button onclick={() => addFeedback('indicator')}>Add status port</button><button onclick={() => addFeedback('gauge')}>Add numeric port</button><button onclick={() => addFeedback('text')}>Add text port</button></div>
      </details>
    </aside>
    <main class="design-stage-panel">
      <div class="state-toolbar"><label>State<select aria-label="Widget preview state" bind:value={previewState}><option value="base">Base appearance</option>{#each WIDGET_STATES as state}<option value={state}>{state}</option>{/each}</select></label><label class="snap"><input type="checkbox" bind:checked={snap} />Snap to grid</label><span>Simulated state preview</span></div>
      <div class="design-stage-wrap"><div class="design-stage" bind:this={stage} style:aspect-ratio={`${draft.size.width}/${draft.size.height}`}>
        <WidgetView definition={draft} {layers} state={previewState === 'base' ? 'normal' : previewState} samples={simulated} {assets} />
        {#if shownBounds && layer}<div class="selected-layer" style:left={`${shownBounds.x * 100}%`} style:top={`${shownBounds.y * 100}%`} style:width={`${shownBounds.width * 100}%`} style:height={`${shownBounds.height * 100}%`}>
          <button class="layer-drag" aria-label={`Move layer ${layer.label}`} onpointerdown={e => begin(e, false)} onpointermove={move} onpointerup={finish} onpointercancel={() => drag = null} onlostpointercapture={() => drag = null}></button>
          <span>{layer.label}</span><button class="layer-resize" aria-label={`Resize layer ${layer.label}`} onpointerdown={e => begin(e, true)} onpointermove={move} onpointerup={finish} onpointercancel={() => drag = null} onlostpointercapture={() => drag = null}>↘</button>
        </div>{/if}
      </div></div>
      <p class="stage-hint">Drag the selected layer or its corner. A saved library revision is immutable; existing panels keep their pinned version until you update them.</p>
      <div class="definition-size"><label>Design width<input type="number" min="32" max="1024" value={draft.size.width} onchange={e => command({ type: 'update', patch: { size: { ...draft.size, width: e.currentTarget.valueAsNumber } } })} /></label><label>Design height<input type="number" min="32" max="1024" value={draft.size.height} onchange={e => command({ type: 'update', patch: { size: { ...draft.size, height: e.currentTarget.valueAsNumber } } })} /></label></div>
    </main>
    <aside class="layer-properties" aria-label="Layer properties">
      {#if layer}
        <h3>{layer.label}</h3><label>Layer name<input value={layer.label} onchange={e => patch({ label: e.currentTarget.value })} /></label>
        <div class="geometry">{#each ['x', 'y', 'width', 'height'] as field}<label>{field} (%)<input type="number" min={field === 'width' || field === 'height' ? .1 : 0} max="100" step=".1" value={Math.round(layer.bounds[field as keyof Bounds] * 1000) / 10} onchange={e => geometry(field as keyof Bounds, e.currentTarget.valueAsNumber)} /></label>{/each}</div>
        <h4>{previewState === 'base' ? 'Base paint' : `${previewState} overrides`}</h4>
        {#each ['fill', 'stroke'] as key}<div class="paint"><label>{key}<input type="color" value={String(literal(paintValue(key), '#63dfd0'))} disabled={!!binding(paintValue(key))} onchange={e => style(key, e.currentTarget.value)} /></label><select aria-label={`${key} property binding`} value={binding(paintValue(key))} onchange={e => style(key, e.currentTarget.value ? { property: e.currentTarget.value } : '#63dfd0')}><option value="">Fixed color</option>{#each draft.properties.filter(p => p.type === 'color') as p}<option value={p.name}>{p.label}</option>{/each}</select></div>{/each}
        <div class="geometry"><label>Stroke width<input type="number" min="0" max="16" step="1" value={Number(literal(paintValue('strokeWidth'), 0))} onchange={e => style('strokeWidth', e.currentTarget.valueAsNumber)} /></label><label>Opacity<input type="number" min="0" max="1" step=".05" value={Number(literal(paintValue('opacity'), 1))} onchange={e => style('opacity', e.currentTarget.valueAsNumber)} /></label></div>
        <label class="snap"><input type="checkbox" checked={paintValue('visible') !== false} onchange={e => style('visible', e.currentTarget.checked)} />Visible</label>
        {#if layer.kind === 'rectangle'}<label>Corner radius<input type="number" min="0" max="64" value={Number(literal(layer.radius, 0))} onchange={e => patch({ radius: e.currentTarget.valueAsNumber })} /></label>{/if}
        {#if layer.kind === 'text'}
          <label>Text<input value={String(literal(layer.text))} disabled={!!binding(layer.text)} onchange={e => patch({ text: e.currentTarget.value })} /></label><label>Text property<select value={binding(layer.text)} onchange={e => patch({ text: e.currentTarget.value ? { property: e.currentTarget.value } : 'Label' })}><option value="">Fixed text</option>{#each draft.properties.filter(p => p.type === 'text') as p}<option value={p.name}>{p.label}</option>{/each}</select></label>
          <div class="geometry"><label>Font<select value={layer.font} onchange={e => patch({ font: e.currentTarget.value })}><option value="sans">Sans</option><option value="mono">Mono</option></select></label><label>Font size<input type="number" min="8" max="128" value={layer.fontSize} onchange={e => patch({ fontSize: e.currentTarget.valueAsNumber })} /></label></div>
          <label>Alignment<select value={layer.align} onchange={e => patch({ align: e.currentTarget.value })}>{#each ['left', 'center', 'right'] as align}<option value={align}>{align}</option>{/each}</select></label>
          <label>Text feedback<select value={layer.outputPort ?? ''} onchange={e => { const next = structuredClone(layer!); if (next.kind !== 'text') return; if (e.currentTarget.value) next.outputPort = e.currentTarget.value; else delete next.outputPort; command({ type: 'replace', definition: { ...draft, layers: draft.layers.map(l => l.id === next.id ? next : l) } }); }}><option value="">No feedback</option>{#each draft.outputs as port}<option value={port.name}>{port.label}</option>{/each}</select></label>
        {:else if layer.kind === 'image'}<label>Image asset<select value={String(literal(layer.assetId))} onchange={e => patch({ assetId: e.currentTarget.value })}><option value="">No image</option>{#each assetOptions as asset}<option value={asset.id}>{asset.label}</option>{/each}</select></label>
        {:else if layer.kind === 'indicator' || layer.kind === 'gauge'}
          <label>Feedback port<select value={layer.outputPort} onchange={e => patch({ outputPort: e.currentTarget.value })}>{#each draft.outputs.filter(p => p.kind === (layer!.kind === 'indicator' ? 'indicator' : 'gauge')) as port}<option value={port.name}>{port.label}</option>{/each}</select></label>
          {#if layer.kind === 'gauge'}<label>Gauge shape<select value={layer.orientation} onchange={e => patch({ orientation: e.currentTarget.value })}><option value="horizontal">Horizontal bar</option><option value="vertical">Vertical bar</option><option value="radial">Radial</option></select></label><div class="geometry"><label>Minimum<input type="number" value={Number(literal(layer.min))} onchange={e => patch({ min: e.currentTarget.valueAsNumber })} /></label><label>Maximum<input type="number" value={Number(literal(layer.max))} onchange={e => patch({ max: e.currentTarget.valueAsNumber })} /></label></div>{/if}
        {/if}
      {/if}
      <details open><summary>Exposed properties</summary>
        {#each draft.properties as property}<div class="exposed-property"><label>Property label<input value={property.label} onchange={e => updateProperty(property, { label: e.currentTarget.value })} /></label><small>{property.name} · {property.type}</small>
          {#if property.type === 'boolean'}<label class="snap"><input type="checkbox" checked={property.default} onchange={e => updateProperty(property, { default: e.currentTarget.checked })} />Default value</label>
          {:else if property.type === 'number'}<label>Default value<input type="number" min={property.min} max={property.max} value={property.default} onchange={e => updateProperty(property, { default: e.currentTarget.valueAsNumber })} /></label>
          {:else if property.type === 'asset'}<label>Default image<select value={property.default} onchange={e => updateProperty(property, { default: e.currentTarget.value })}><option value="">None</option>{#each assetOptions as asset}<option value={asset.id}>{asset.label}</option>{/each}</select></label>
          {:else}<label>Default value<input type={property.type === 'color' ? 'color' : 'text'} value={property.default} onchange={e => updateProperty(property, { default: e.currentTarget.value })} /></label>{/if}
        </div>{/each}
        <div class="add-layer"><select aria-label="New property type" bind:value={propertyType}>{#each ['text', 'color', 'number', 'boolean', 'asset'] as type}<option value={type}>{type}</option>{/each}</select><button onclick={addProperty}>Add property</button></div>
      </details>
    </aside>
  </div>
</dialog>

<style>
  .widget-designer{width:min(1360px,96vw);height:min(900px,94vh);max-width:none;max-height:none;margin:auto;padding:0;border:1px solid #36505b;border-radius:12px;background:#111c25;color:#deecef;box-shadow:0 24px 100px #000a;font:14px inherit}.widget-designer::backdrop{background:#050b11bf}.designer-heading{display:flex;justify-content:space-between;align-items:center;gap:20px;padding:22px 26px;border-bottom:1px solid #2a3b47}.eyebrow{font-size:10px;letter-spacing:.15em;color:#83c6b4}h2{margin:5px 0;font-size:23px}.designer-heading p{margin:0;font-size:12px;color:#8fa5b2}.designer-actions{display:flex;gap:8px;flex-wrap:wrap}button,input,select{font:inherit;color:inherit}button{padding:8px 10px;border:1px solid #354c58;border-radius:5px;background:#20313f;cursor:pointer}button:hover{border-color:#70bca7}button:disabled{opacity:.4;cursor:default}.primary{background:#264c3e;border-color:#588e76}input,select{width:100%;min-width:0;padding:7px 8px;border:1px solid #344a58;background:#14232f;border-radius:4px;box-sizing:border-box}input[type=color]{height:34px;padding:3px}input[type=checkbox]{width:auto}label{display:flex;flex-direction:column;gap:5px;font-size:11px;color:#b4c6ce;margin:10px 0}h3{font-size:13px;margin:18px 0 10px}h4{font-size:11px;margin:18px 0 8px;color:#8bb9aa}small{font-size:10px;color:#839ba9;font-weight:400}h3 small{display:block;margin-top:5px}.designer-workspace{display:grid;grid-template-columns:225px minmax(300px,1fr) 280px;height:calc(100% - 105px);min-height:0}.layers-panel,.layer-properties{padding:14px 18px;overflow:auto;min-height:0}.layers-panel{border-right:1px solid #2a3b47}.layer-properties{border-left:1px solid #2a3b47}.layers-panel ol{padding:0;margin:0;list-style:none}.layers-panel li button{display:grid;grid-template-columns:18px 1fr;gap:4px;text-align:left;width:100%;margin:3px 0;background:#162734;border-color:transparent}.layers-panel li button.selected{border-color:#79bca8;background:#214036}.layers-panel li strong{font-size:11px;font-weight:500;overflow:hidden;text-overflow:ellipsis}.layers-panel li small{grid-column:2}.layers-panel li span{color:#78969c;font-size:10px}.add-layer,.geometry,.paint{display:grid;grid-template-columns:1fr 1fr;gap:8px;align-items:end}.add-layer{margin-top:12px}.add-layer button{font-size:11px}.layer-actions{display:grid;grid-template-columns:1fr 1fr;gap:5px;margin:12px 0}.layer-actions button{font-size:10px;padding:6px}.design-stage-panel{padding:16px 24px;overflow:auto;background:#0d1720;min-width:0}.state-toolbar{display:flex;align-items:center;gap:18px;flex-wrap:wrap;margin-bottom:20px}.state-toolbar>span{font-size:10px;color:#8d9da8}.state-toolbar label{margin:0}.snap{display:flex;flex-direction:row;align-items:center;gap:7px}.design-stage-wrap{display:flex;align-items:center;justify-content:center;min-height:260px;padding:35px 20px;background-image:linear-gradient(#344c551a 1px,transparent 1px),linear-gradient(90deg,#344c551a 1px,transparent 1px);background-size:20px 20px;border:1px solid #28404e;border-radius:7px}.design-stage{position:relative;width:100%;max-height:440px}.selected-layer{position:absolute;border:1px solid #a4dacf;box-sizing:border-box;pointer-events:none}.selected-layer>span{position:absolute;left:0;top:-17px;font-size:10px;color:#d0f0e6;white-space:nowrap;background:#25483d;padding:2px 5px}.layer-drag{position:absolute;inset:0;width:100%;height:100%;border:0;background:transparent;cursor:move;touch-action:none;pointer-events:auto}.layer-resize{position:absolute;bottom:-7px;right:-7px;width:20px;height:20px;padding:0;background:#315f50;border:1px solid #96d2c3;border-radius:2px;cursor:nwse-resize;touch-action:none;pointer-events:auto}.stage-hint{font-size:11px;color:#849eaa;line-height:1.6;margin:18px 0}.definition-size{display:grid;grid-template-columns:1fr 1fr;gap:12px}.geometry label{min-width:0}.paint label{margin:5px 0}.paint select{margin-bottom:5px;font-size:10px}.exposed-property,.port{padding:8px 0;border-bottom:1px solid #263d49}.exposed-property label{margin:5px 0}.designer-error{padding:10px 24px;margin:0;background:#542b2b;color:#ffd0bd;font-size:12px}details{margin:15px 0}summary{font-size:12px;color:#aec8d0;cursor:pointer}button:focus-visible,input:focus-visible,select:focus-visible{outline:2px solid #9bd4c3;outline-offset:2px}@media(max-width:1000px){.designer-workspace{grid-template-columns:180px minmax(250px,1fr) 230px}.designer-heading{padding:14px}.designer-actions button{font-size:11px}.design-stage-panel{padding:15px}}@media(max-width:750px){.designer-workspace{grid-template-columns:160px 1fr}.layer-properties{grid-column:1/-1;border-left:0}.designer-heading{align-items:flex-start}.designer-heading p{display:none}.designer-actions{max-width:230px}.widget-designer{overflow:auto}.designer-workspace{height:auto}}
</style>
