<script lang="ts">
  import { onMount } from 'svelte';
  import type { Bounds } from '@opensimgear/companion-contracts';
  import { boundsContain, boundsOverlap, viewportRegions, type DesignerDocument, type DesignerWidget } from '@opensimgear/companion-designer';
  import { axisPosition, createPreviewController, type PreviewController, type PreviewSnapshot, type PointerData, type PreviewChange } from './controller.ts';
  import { feedbackPresentation, movedBounds, resizedBounds, safeAssetUrl, viewportWidgetBounds, type FeedbackSamples } from './presentation.ts';
  interface Props {
    document: DesignerDocument; pageId?: string; assets?: Record<string, string>; mode?: 'edit' | 'preview'; selectedIds?: string[];
    controller?: PreviewController; feedback?: FeedbackSamples; oncontroller?: (controller: PreviewController) => void;
    onselect?: (ids: string[]) => void; onlayout?: (widgetId: string, bounds: Bounds) => void;
    onchange?: (changes: PreviewChange[], reason: string) => void;
    selectedViewportId?: string; onselectviewport?: (id: string) => void;
    onviewportbounds?: (id: string, bounds: Bounds) => void; onerror?: (message: string) => void;
  }
  let { document, pageId, assets = {}, mode = 'preview', selectedIds = [], controller, feedback = {}, oncontroller, onselect, onlayout, onchange, selectedViewportId, onselectviewport, onviewportbounds, onerror }: Props = $props();
  let local = $state<PreviewController | undefined>();
  const engine = $derived(controller ?? local);
  let previewState = $state<PreviewSnapshot>({ armed: false, activePointers: 0, values: {}, events: [], reason: 'Disarmed' });
  let now = $state(performance.now());
  const selectedPage = $derived(pageId ?? document.ui.pages[0]?.id);
  const viewport = $derived(document.presentation.viewports.find(item => item.pageId === selectedPage));
  const widgets = $derived(document.presentation.widgets.filter(item => item.pageId === selectedPage));
  const groups = $derived(document.presentation.groups.filter(item => item.pageId === selectedPage));
  const regions = $derived(viewportRegions(document, selectedPage ?? ''));
  const activeViewport = $derived(selectedViewportId ?? regions[0]?.id);
  const background = $derived(viewport?.backgroundAssetId && document.ui.assets.some(asset => asset.id === viewport.backgroundAssetId) ? safeAssetUrl(assets[viewport.backgroundAssetId]) : null);
  const insets = $derived(viewport?.safeInsets ?? { top: 0, right: 0, bottom: 0, left: 0 });
  let content: HTMLDivElement;
  let contentWidth = $state(0);
  let contentHeight = $state(0);
  let drag = $state<{ pointerId: number; id: string; x: number; y: number; bounds: Bounds; preview: Bounds; resize: boolean; moved: boolean; shift: boolean } | null>(null);
  let viewportDrag = $state<{ pointerId: number; id: string; x: number; y: number; bounds: Bounds; preview: Bounds; resize: boolean; moved: boolean } | null>(null);
  const shownRegions = $derived(regions.map(region => ({ ...region, bounds: viewportDrag?.id === region.id ? viewportDrag.preview : region.bounds })));
  const viewportError = $derived(viewportDrag ? viewportLayoutError(viewportDrag.id, viewportDrag.preview) : '');
  const directions = [{ label: 'Up left', icon: '↖', value: 7 }, { label: 'Up', icon: '↑', value: 0 }, { label: 'Up right', icon: '↗', value: 1 }, { label: 'Left', icon: '←', value: 6 }, { label: 'Neutral', icon: '•', value: -1 }, { label: 'Right', icon: '→', value: 2 }, { label: 'Down left', icon: '↙', value: 5 }, { label: 'Down', icon: '↓', value: 4 }, { label: 'Down right', icon: '↘', value: 3 }];
  const symbols = { power: '⏻', warning: '⚠', gear: '⚙', check: '✓' };
  onMount(() => {
    local = createPreviewController(document, { onchange: (changes, reason) => onchange?.(changes, reason) });
    oncontroller?.(controller ?? local);
    const observer = new ResizeObserver(() => { contentWidth = content.clientWidth; contentHeight = content.clientHeight; });
    observer.observe(content);
    const timer = setInterval(() => { now = performance.now(); if (engine?.snapshot().armed) engine.tick(now); }, 20);
    return () => { observer.disconnect(); clearInterval(timer); engine?.disarm('Renderer unmounted'); };
  });
  $effect(() => { if (engine) return engine.subscribe(snapshot => { previewState = snapshot; }); });
  $effect(() => { engine?.setDocument(document); });
  $effect(() => { selectedPage; mode; engine?.disarm('Page or mode changed'); drag = null; viewportDrag = null; });
  function stop(reason: string) { engine?.disarm(reason); drag = null; viewportDrag = null; }
  function visibility() { if (globalThis.document.visibilityState === 'hidden') stop('Page hidden'); }
  function position(event: PointerEvent, widget: DesignerWidget): PointerData {
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: (event.clientX - bounds.left) / bounds.width, y: 1 - (event.clientY - bounds.top) / bounds.height };
  }
  function start(event: PointerEvent, widget: DesignerWidget, data?: PointerData) {
    if (mode !== 'preview' || (event.pointerType === 'mouse' && event.button !== 0)) return;
    event.preventDefault();
    if (engine?.begin(event.pointerId, widget.id, data ?? position(event, widget))) {
      try { (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId); }
      catch { engine.release(event.pointerId, 'Pointer capture unavailable', true); }
    }
  }
  function finish(event: PointerEvent, cancelled = false) { engine?.release(event.pointerId, cancelled ? 'Pointer cancelled or capture lost' : 'Pointer released', cancelled); }
  function keyboard(event: KeyboardEvent, widget: DesignerWidget, down: boolean, data: PointerData = {}, offset = 0) {
    if (![' ', 'Enter'].includes(event.key)) return;
    event.preventDefault();
    const pointer = -1 - document.presentation.widgets.indexOf(widget) * 16 - offset;
    if (down && !event.repeat && mode === 'preview') engine?.begin(pointer, widget.id, data);
    if (!down) engine?.release(pointer, 'Key released');
  }
  function keyboardBlur(widget: DesignerWidget, offset = 0, cancelled = true) { engine?.release(-1 - document.presentation.widgets.indexOf(widget) * 16 - offset, cancelled ? 'Control lost focus' : 'Key released', cancelled); if (cancelled) engine?.cancelWidget(widget.id); }
  function choose(widget: DesignerWidget, value: number) {
    const pointer = -10000;
    if (mode === 'preview' && engine?.begin(pointer, widget.id, { value })) engine.release(pointer);
  }
  function axisKey(event: KeyboardEvent, widget: DesignerWidget) {
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key) || mode !== 'preview' || !previewState.armed) return;
    event.preventDefault();
    const current = widget.controlIds.map(id => {
      const control = document.ui.controls.find(control => control.id === id);
      const value = Number(previewState.values[id] ?? 0);
      return control?.kind === 'axis' && control.axisMode === 'bipolar' ? (value + 1) / 2 : value;
    });
    const axis = widget.kind === 'xy' && ['ArrowUp', 'ArrowDown'].includes(event.key) ? 1 : 0;
    current[axis] = event.key === 'Home' ? 0 : event.key === 'End' ? 1 : Math.max(0, Math.min(1, current[axis] + (['ArrowRight', 'ArrowUp'].includes(event.key) ? .05 : -.05)));
    const pointer = -1 - document.presentation.widgets.indexOf(widget) * 16;
        if (widget.kind !== 'slider' && widget.kind !== 'xy') return;
    const positions = current.map((unit, index) => {
      const control = document.ui.controls.find(control => control.id === widget.controlIds[index]);
      const mode = control?.kind === 'axis' ? control.axisMode : 'bipolar';
      const inverted = widget.kind === 'slider' ? widget.config.inverted : index === 0 ? widget.config.invertX : widget.config.invertY;
      return axisPosition(mode === 'bipolar' ? unit * 2 - 1 : unit, mode, widget.config.deadzone, inverted);
    });
    const data = { x: positions[0], y: widget.kind === 'xy' ? positions[1] : positions[0] };
    if (!engine?.move(pointer, data)) engine?.begin(pointer, widget.id, data);
  }
  function editStart(event: PointerEvent, widget: DesignerWidget, resize = false) {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    event.preventDefault(); event.stopPropagation();
    drag = { pointerId: event.pointerId, id: widget.id, x: event.clientX, y: event.clientY, bounds: { ...widget.bounds }, preview: { ...widget.bounds }, resize, moved: false, shift: event.shiftKey };
    try { (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId); } catch { drag = null; }
  }
  function editMove(event: PointerEvent) {
    if (!drag || event.pointerId !== drag.pointerId || !content) return;
    if (!drag.moved && Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 4) return;
    drag.moved = true;
    if (!onlayout) return;
    const rect = content.getBoundingClientRect();
    drag.preview = (drag.resize ? resizedBounds : movedBounds)(drag.bounds, (event.clientX - drag.x) / rect.width, (event.clientY - drag.y) / rect.height, document.presentation.grid);
  }
  function editFinish(event: PointerEvent) {
    if (!drag || drag.pointerId !== event.pointerId) return;
    const completed = drag; drag = null;
    if (completed.moved) {
      if (boundsChanged(completed.bounds, completed.preview)) applyWidgetLayout(completed.id, completed.preview);
    } else if (!completed.resize) {
      onselect?.(selectedIds.includes(completed.id) ? selectedIds.filter(id => id !== completed.id) : completed.shift ? [...selectedIds, completed.id] : [completed.id]);
    }
  }
  function shownBounds(widget: DesignerWidget) {
    if (drag?.id === widget.id) return drag.preview;
    if (viewportDrag && boundsContain(viewportDrag.bounds, widget.bounds)) return viewportWidgetBounds(widget.bounds, viewportDrag.bounds, viewportDrag.preview);
    return widget.bounds;
  }
  function widgetFits(bounds: Bounds) { return shownRegions.some(region => boundsContain(region.bounds, bounds)); }
  function boundsChanged(before: Bounds, after: Bounds) { return before.x !== after.x || before.y !== after.y || before.width !== after.width || before.height !== after.height; }
  function applyWidgetLayout(id: string, bounds: Bounds) {
    if (!widgetFits(bounds)) { onerror?.('Widgets must stay fully inside a viewport. Move or resize the widget to fit within one viewport.'); return; }
    onlayout?.(id, { ...bounds });
  }
  function selectViewport(id: string) {
    if (selectedIds.length) onselect?.([]);
    else onselectviewport?.(id);
  }
  function canvasRelease(event: PointerEvent) {
    if (mode !== 'edit' || event.button !== 0 || (event.target as HTMLElement).closest('.widget,.viewport-hit,.viewport-outline')) return;
    onselect?.([]);
  }
  function viewportLayoutError(id: string, bounds: Bounds) {
    if (regions.some(region => region.id !== id && boundsOverlap(region.bounds, bounds))) return 'Viewports must not overlap. Move or resize this viewport into an empty area.';
    return '';
  }
  function applyViewportLayout(id: string, bounds: Bounds) {
    const error = viewportLayoutError(id, bounds);
    if (error) { onerror?.(error); return; }
    onviewportbounds?.(id, { ...bounds });
  }
  function viewportStart(event: PointerEvent, id: string, bounds: Bounds, resize = false) {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    event.preventDefault(); event.stopPropagation();
    viewportDrag = { pointerId: event.pointerId, id, x: event.clientX, y: event.clientY, bounds: { ...bounds }, preview: { ...bounds }, resize, moved: false };
    try { (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId); } catch { viewportDrag = null; }
  }
  function viewportMove(event: PointerEvent) {
    if (!viewportDrag || event.pointerId !== viewportDrag.pointerId || !content) return;
    if (!viewportDrag.moved && Math.hypot(event.clientX - viewportDrag.x, event.clientY - viewportDrag.y) < 4) return;
    viewportDrag.moved = true;
    if (!onviewportbounds) return;
    const rect = content.getBoundingClientRect();
    viewportDrag.preview = (viewportDrag.resize ? resizedBounds : movedBounds)(viewportDrag.bounds, (event.clientX - viewportDrag.x) / rect.width, (event.clientY - viewportDrag.y) / rect.height, document.presentation.grid);
  }
  function viewportFinish(event: PointerEvent) {
    if (!viewportDrag || event.pointerId !== viewportDrag.pointerId) return;
    const completed = viewportDrag; viewportDrag = null;
    if (completed.moved) {
      if (boundsChanged(completed.bounds, completed.preview)) applyViewportLayout(completed.id, completed.preview);
    } else if (!completed.resize) selectViewport(completed.id);
  }
  function viewportKey(event: KeyboardEvent, id: string, bounds: Bounds, resize = false) {
    if ([' ', 'Enter'].includes(event.key)) { event.preventDefault(); selectViewport(id); return; }
    const step = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[event.key];
    if (!step) return;
    event.preventDefault(); event.stopPropagation();
    const grid = document.presentation.grid;
    applyViewportLayout(id, (resize ? resizedBounds : movedBounds)(bounds, step[0] / grid.columns, step[1] / grid.rows, grid));
  }
  function resizeKey(event: KeyboardEvent, widget: DesignerWidget) {
    const step = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[event.key];
    if (!step) return;
    event.preventDefault();
    const grid = document.presentation.grid;
    applyWidgetLayout(widget.id, resizedBounds(widget.bounds, step[0] / grid.columns, step[1] / grid.rows, grid));
  }
  function editKey(event: KeyboardEvent, widget: DesignerWidget) {
    if ([' ', 'Enter'].includes(event.key)) { event.preventDefault(); onselect?.([widget.id]); return; }
    const move = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[event.key];
    if (!move) return;
    event.preventDefault();
    const grid = document.presentation.grid;
    applyWidgetLayout(widget.id, { ...widget.bounds, x: Math.max(0, Math.min(1 - widget.bounds.width, widget.bounds.x + move[0] / grid.columns)), y: Math.max(0, Math.min(1 - widget.bounds.height, widget.bounds.y + move[1] / grid.rows)) });
  }
  function groupBounds(id: string) {
    const members = widgets.filter(widget => widget.groupId === id).map(widget => ({ ...widget, bounds: shownBounds(widget) }));
    if (!members.length) return 'display:none';
    const x = Math.min(...members.map(item => item.bounds.x)), y = Math.min(...members.map(item => item.bounds.y));
    const right = Math.max(...members.map(item => item.bounds.x + item.bounds.width)), bottom = Math.max(...members.map(item => item.bounds.y + item.bounds.height));
    return `left:${x * 100}%;top:${y * 100}%;width:${(right - x) * 100}%;height:${(bottom - y) * 100}%`;
  }
  function dimensions(widget: DesignerWidget) {
    const bounds = shownBounds(widget);
    const width = Math.round(bounds.width * (viewport?.width ?? 1280) * (1 - insets.left - insets.right));
    const height = Math.round(bounds.height * (viewport?.height ?? 800) * (1 - insets.top - insets.bottom));
    const displayWidth = bounds.width * contentWidth, displayHeight = bounds.height * contentHeight;
    return { width, height, small: widget.controlIds.length > 0 && (displayWidth < 44 || displayHeight < 44) };
  }
  function controlPosition(widget: DesignerWidget, index = 0) {
    const control = document.ui.controls.find(item => item.id === widget.controlIds[index]);
    const value = Number(previewState.values[widget.controlIds[index]] ?? 0);
    return control?.kind === 'axis' && control.axisMode === 'bipolar' ? (value + 1) / 2 : value;
  }
</script>

<svelte:window onblur={() => stop('Window lost focus')} onpagehide={() => stop('Page unloading')} />
<svelte:document onvisibilitychange={visibility} />
<div class="panel-canvas" role="region" onpointerup={canvasRelease} class:editing={mode === 'edit'} class:disarmed={!previewState.armed && mode === 'preview'} style:aspect-ratio={`${viewport?.width ?? 1280}/${viewport?.height ?? 800}`} style:background-image={background ? `url("${background}")` : undefined} aria-label={`${document.ui.title} panel`}>
  <div class="safe-area" style:top={`${insets.top * 100}%`} style:right={`${insets.right * 100}%`} style:bottom={`${insets.bottom * 100}%`} style:left={`${insets.left * 100}%`} bind:this={content} style:--grid-x={`${100 / document.presentation.grid.columns}%`} style:--grid-y={`${100 / document.presentation.grid.rows}%`}>{#if mode === 'edit'}<span class="safe-label">CONTENT AREA · {viewport?.width ?? 1280} × {viewport?.height ?? 800}</span>{/if}
    {#if mode === 'edit'}
      {#each shownRegions as region (region.id)}
        <button class="viewport-hit" class:viewport-selected={activeViewport === region.id} style:left={`${region.bounds.x * 100}%`} style:top={`${region.bounds.y * 100}%`} style:width={`${region.bounds.width * 100}%`} style:height={`${region.bounds.height * 100}%`} aria-label={`Select viewport ${region.label}`} aria-pressed={activeViewport === region.id} onclick={() => selectViewport(region.id)}></button>
        <div class="viewport-outline" class:viewport-selected={activeViewport === region.id} class:viewport-invalid={viewportDrag?.id === region.id && Boolean(viewportError)} style:left={`${region.bounds.x * 100}%`} style:top={`${region.bounds.y * 100}%`} style:width={`${region.bounds.width * 100}%`} style:height={`${region.bounds.height * 100}%`}>
          <button class="viewport-label" aria-label={`Select or move viewport ${region.label}`} aria-pressed={activeViewport === region.id} title="Drag to move viewport; arrow keys adjust by grid steps" onpointerdown={(event) => viewportStart(event, region.id, region.bounds)} onpointermove={viewportMove} onpointerup={viewportFinish} onpointercancel={() => { viewportDrag = null; }} onlostpointercapture={() => { viewportDrag = null; }} onkeydown={(event) => viewportKey(event, region.id, region.bounds)}>{region.label}</button>
          {#if activeViewport === region.id}<button class="viewport-resize" aria-label={`Resize viewport ${region.label}`} title="Drag to resize viewport; arrow keys adjust by grid steps" onpointerdown={(event) => viewportStart(event, region.id, region.bounds, true)} onpointermove={viewportMove} onpointerup={viewportFinish} onpointercancel={() => { viewportDrag = null; }} onlostpointercapture={() => { viewportDrag = null; }} onkeydown={(event) => viewportKey(event, region.id, region.bounds, true)}>↘</button>{/if}
        </div>
      {/each}
      {#if viewportError}<span class="viewport-error" role="status">{viewportError}</span>{/if}
    {/if}
    {#each groups as group}<div class="group-outline" style={groupBounds(group.id)}><span>{group.label}</span></div>{/each}
    {#each widgets as widget (widget.id)}
      {@const display = feedbackPresentation(widget, document.ui, feedback, now)}
      {@const size = dimensions(widget)}
      {@const bounds = shownBounds(widget)}
      <section class="widget" class:selected={selectedIds.includes(widget.id)} class:output={widget.feedbackIds.length > 0} class:feedback-unavailable={widget.feedbackIds.length > 0 && !display.available} class:touch-warning={mode === 'edit' && size.small} class:outside-viewport={mode === 'edit' && !widgetFits(bounds)}
        style:left={`${bounds.x * 100}%`} style:top={`${bounds.y * 100}%`} style:width={`${bounds.width * 100}%`} style:height={`${bounds.height * 100}%`}
        style:color={widget.style?.foreground ?? '#dce7ef'} style:background={widget.style?.background ?? '#152532'} style:--widget-accent={widget.style?.accent ?? '#83e3c2'} aria-label={mode === 'edit' ? `${widget.label}, ${size.width} by ${size.height} pixels${size.small ? ", below 44 pixel touch target" : ""}` : widget.label}>
        <header><span>{widget.label}</span><small>{widget.kind}</small></header>
        <div class="widget-body" inert={mode === 'edit' || (widget.controlIds.length > 0 && !previewState.armed)}>
          {#if widget.kind === 'button'}
            <button class="press-button" class:active={Boolean(previewState.values[widget.controlIds[0]])} aria-pressed={widget.config.buttonMode === 'toggle' ? Boolean(previewState.values[widget.controlIds[0]]) : undefined}
              onpointerdown={(event) => start(event, widget)} onpointerup={(event) => finish(event)} onpointercancel={(event) => finish(event, true)} onlostpointercapture={(event) => finish(event, true)}
              onkeydown={(event) => keyboard(event, widget, true)} onkeyup={(event) => keyboard(event, widget, false)} onblur={() => keyboardBlur(widget)}>
              {#if widget.config.iconAssetId && document.ui.assets.some(asset => asset.id === widget.config.iconAssetId) && safeAssetUrl(assets[widget.config.iconAssetId])}<img class="button-icon" src={safeAssetUrl(assets[widget.config.iconAssetId])!} alt="" />{/if}
              {widget.label}<small>{widget.config.buttonMode}</small></button>
          {:else if widget.kind === 'selector'}
            <select aria-label={widget.label} value={Number(previewState.values[widget.controlIds[0]])} onchange={(event) => choose(widget, Number(event.currentTarget.value))}>{#each widget.config.options as option}<option value={option.value}>{option.label}</option>{/each}</select>
          {:else if widget.kind === 'hat'}
            <div class="hat">{#each directions as direction}{#if direction.value < 0}<span class="hat-neutral">•</span>{:else}<button aria-label={`${widget.label}: ${direction.label}`} class:active={previewState.values[widget.controlIds[0]] === direction.value}
              onpointerdown={(event) => start(event, widget, { value: direction.value })} onpointerup={(event) => finish(event)} onpointercancel={(event) => finish(event, true)} onlostpointercapture={(event) => finish(event, true)}
              onkeydown={(event) => keyboard(event, widget, true, { value: direction.value }, direction.value)} onkeyup={(event) => keyboard(event, widget, false, {}, direction.value)} onblur={() => keyboardBlur(widget, direction.value)}>{direction.icon}</button>{/if}{/each}</div>
          {:else if widget.kind === 'encoder'}
            <div class="encoder">{#each [-1, 1] as direction, index}<button aria-label={`${widget.label}: ${direction < 0 ? 'decrease' : 'increase'}`} onpointerdown={(event) => start(event, widget, { direction: direction as -1 | 1 })} onpointerup={(event) => finish(event)} onpointercancel={(event) => finish(event, true)} onlostpointercapture={(event) => finish(event, true)}
              onkeydown={(event) => keyboard(event, widget, true, { direction: direction as -1 | 1 }, index)} onkeyup={(event) => keyboard(event, widget, false, {}, index)} onblur={() => keyboardBlur(widget, index)}>{direction < 0 ? '−' : '+'}</button>{/each}</div>
          {:else if widget.kind === 'slider' || widget.kind === 'xy'}
            <div class="axis-control" class:xy={widget.kind === 'xy'} class:vertical={widget.kind === 'slider' && widget.config.orientation === 'vertical'} role="slider" tabindex="0" aria-label={widget.label} aria-valuemin={0} aria-valuemax={1} aria-valuenow={controlPosition(widget)} aria-valuetext={widget.controlIds.map(id => `${id}: ${previewState.values[id] ?? 0}`).join(', ')}
              onpointerdown={(event) => start(event, widget)} onpointermove={(event) => engine?.move(event.pointerId, position(event, widget))} onpointerup={(event) => finish(event)} onpointercancel={(event) => finish(event, true)} onlostpointercapture={(event) => finish(event, true)}
              onkeydown={(event) => axisKey(event, widget)} onkeyup={(event) => { if (event.key.startsWith('Arrow') || ['Home', 'End'].includes(event.key)) keyboardBlur(widget, 0, false); }} onblur={() => keyboardBlur(widget)}>
              <span class="axis-line horizontal"></span>{#if widget.kind === 'xy'}<span class="axis-line vertical"></span>{/if}
              <span class="axis-thumb" style:left={`${widget.kind === 'slider' && widget.config.orientation === 'vertical' ? 50 : controlPosition(widget) * 100}%`} style:top={`${widget.kind === 'xy' ? (1 - controlPosition(widget, 1)) * 100 : widget.kind === 'slider' && widget.config.orientation === 'vertical' ? (1 - controlPosition(widget)) * 100 : 50}%`}></span>
              <output>{widget.controlIds.map(id => Number(previewState.values[id] ?? 0).toFixed(2)).join(' / ')}</output>
            </div>
          {:else if widget.kind === 'icon'}
            {@const asset = widget.config.iconAssetId && document.ui.assets.some(item => item.id === widget.config.iconAssetId) ? safeAssetUrl(assets[widget.config.iconAssetId]) : null}
            <div class="icon-value" class:on={display.value === true}>{#if asset}<img src={asset} alt={widget.label} />{:else}{symbols[widget.config.symbol]}{/if}</div>
          {:else if widget.kind === 'indicator'}<div class="indicator-value" class:on={display.value === true}><span></span><output>{display.text}</output></div>
          {:else if widget.kind === 'gauge'}<div class="gauge-value"><meter min={widget.config.min} max={widget.config.max} value={typeof display.value === 'number' ? display.value : widget.config.min} aria-label={mode === 'edit' ? `${widget.label}, ${size.width} by ${size.height} pixels${size.small ? ", below 44 pixel touch target" : ""}` : widget.label}></meter><output>{display.text}</output></div>
          {:else}<output class:numeric={widget.kind === 'numeric'}>{display.text}</output>{/if}
        </div>
        {#if widget.feedbackIds.length}<footer class:stale={!display.available}><span>{display.provenance}</span><span>{display.freshness}{display.available ? ` · ${Math.round(display.ageMs)}ms` : ''}</span></footer>{/if}
        {#if mode === 'edit'}<span class="size-badge" class:warning={size.small}>{size.width} × {size.height}px{size.small ? ' · touch < 44px' : ''}</span><button class="edit-hit" aria-label={`Select or move ${widget.label}`} onpointerdown={(event) => editStart(event, widget)} onpointermove={editMove} onpointerup={editFinish} onpointercancel={() => { drag = null; }} onlostpointercapture={() => { drag = null; }} onkeydown={(event) => editKey(event, widget)}></button>{#if selectedIds.includes(widget.id)}<button class="resize-hit" aria-label={`Resize ${widget.label}`} title="Drag to resize; arrow keys adjust by grid steps" onpointerdown={(event) => editStart(event, widget, true)} onpointermove={editMove} onpointerup={editFinish} onpointercancel={() => { drag = null; }} onlostpointercapture={() => { drag = null; }} onkeydown={(event) => resizeKey(event, widget)}>↘</button>{/if}{/if}
      </section>
    {/each}
  </div>
</div>

<style>
  .panel-canvas{background-position:center;background-size:cover;position:relative;width:100%;min-height:240px;background:#0c1720;border:1px solid #30424f;border-radius:10px;overflow:hidden;font-family:inherit;color:#dce7ef}.safe-area{position:absolute}.editing .safe-area{outline:1px dashed #54746899;background-image:linear-gradient(to right,#5a85711a 1px,transparent 1px),linear-gradient(to bottom,#5a85711a 1px,transparent 1px);background-size:var(--grid-x) var(--grid-y)}.safe-label{position:absolute;top:1px;right:3px;z-index:5;font:7px ui-monospace,monospace;color:#8fb09f;pointer-events:none;background:#102019cc}.size-badge{position:absolute;bottom:1px;right:2px;font:7px ui-monospace,monospace;background:#0c1720dd;padding:2px 3px;z-index:4;pointer-events:none;color:#9ebcad}.size-badge.warning{color:#ffd28b}.widget.touch-warning{border-color:#aa7d42}.group-outline{position:absolute;border:1px dashed #70978c55;pointer-events:none;border-radius:8px;transform:scale(1.015)}.group-outline>span{position:absolute;top:-13px;left:7px;font-size:9px;color:#9cb4aa;background:#0c1720;padding:1px 4px}.widget{position:absolute;border:1px solid #3a4f5e;border-radius:7px;padding:7px;display:flex;flex-direction:column;min-width:0;min-height:0;box-sizing:border-box;overflow:hidden}.widget.selected{outline:2px solid #83e3c2;outline-offset:2px;z-index:2}.widget>header{display:flex;align-items:center;justify-content:space-between;gap:6px;font-size:clamp(8px,1vw,12px);margin-bottom:5px;overflow:hidden;flex-shrink:0}.widget>header>span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.widget small{font:8px ui-monospace,monospace;opacity:.6}.widget-body{min-height:0;flex:1;display:flex;align-items:center;justify-content:center;overflow:hidden}.widget-body[inert]{opacity:.62}.widget button,.widget select{font:inherit;color:inherit;box-sizing:border-box;background:#203441;border:1px solid #426071;border-radius:5px;cursor:pointer;touch-action:none;user-select:none}.widget button:focus-visible,.widget select:focus-visible,.axis-control:focus-visible{outline:2px solid var(--widget-accent);outline-offset:1px}.widget button.active{background:color-mix(in srgb,var(--widget-accent) 25%,#142430);border-color:var(--widget-accent)}.press-button{width:100%;height:100%;display:flex;flex-direction:column;justify-content:center;align-items:center;gap:7px;font-size:clamp(9px,1.4vw,18px)!important;padding:4px}.button-icon{width:24px;height:24px;object-fit:contain}.axis-control.vertical .axis-line.horizontal{width:1px;height:auto;top:0;bottom:0;left:50%;right:auto}.widget select{width:100%;padding:8px;font-size:clamp(9px,1.2vw,15px)}.hat{height:100%;aspect-ratio:1;display:grid;grid-template-columns:repeat(3,1fr);grid-template-rows:repeat(3,1fr);gap:3px;max-width:100%}.hat button{padding:0;font-size:clamp(10px,1.8vw,22px)}.hat-neutral{display:grid;place-items:center;color:var(--widget-accent)}.encoder{display:flex;align-items:center;justify-content:center;gap:8px;width:100%;height:100%}.encoder button{width:46%;height:90%;font-size:clamp(15px,3vw,34px)}.axis-control{position:relative;width:100%;height:100%;min-height:20px;background:#0c1b26;border:1px solid #3a5e69;border-radius:5px;touch-action:none;user-select:none;overflow:hidden;box-sizing:border-box;cursor:crosshair}.axis-line{position:absolute;background:#335563;pointer-events:none}.axis-line.horizontal{height:1px;left:0;right:0;top:50%}.axis-line.vertical{width:1px;top:0;bottom:0;left:50%}.axis-thumb{width:14px;height:14px;position:absolute;background:var(--widget-accent);border:3px solid #386358;transform:translate(-50%,-50%);border-radius:50%;pointer-events:none;box-sizing:border-box}.axis-control output{position:absolute;bottom:3px;right:5px;font:9px ui-monospace,monospace;color:#9ebfb6;pointer-events:none}.widget output{font-size:clamp(10px,1.6vw,22px);overflow:hidden;text-overflow:ellipsis;overflow-wrap:anywhere}.widget output.numeric{font-family:ui-monospace,monospace}.icon-value{font-size:clamp(18px,4vw,50px);opacity:.4}.icon-value.on{color:var(--widget-accent);opacity:1}.icon-value img{width:100%;height:100%;max-height:100%;object-fit:contain}.indicator-value{display:flex;align-items:center;justify-content:center;gap:8px}.indicator-value>span{width:13px;height:13px;background:#52616a;border-radius:50%;flex-shrink:0}.indicator-value.on>span{background:var(--widget-accent);box-shadow:0 0 12px #83e3c244}.gauge-value{width:100%;height:100%;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:8px}.gauge-value meter{width:95%;height:22px;accent-color:var(--widget-accent)}.widget>footer{font:7px ui-monospace,monospace;display:flex;justify-content:space-between;gap:3px;margin-top:5px;color:#87a699;flex-shrink:0;overflow:hidden;white-space:nowrap}.widget>footer.stale{color:#e9bb80}.feedback-unavailable .widget-body{opacity:.65}.feedback-unavailable meter{opacity:.3}.edit-hit{position:absolute;inset:0;width:100%;height:100%;border:0!important;background:transparent!important;border-radius:0!important;cursor:move!important;z-index:3}.resize-hit{position:absolute;right:0;bottom:0;width:22px;height:22px;background:#214538!important;border:1px solid #83e3c2!important;border-radius:5px 0 0 0!important;z-index:6;cursor:nwse-resize!important;padding:0;color:#b3efd8!important;font-size:13px!important}.disarmed .widget:not(.output){filter:saturate(.6)}@media(max-width:500px){.widget{padding:4px;border-radius:4px}.widget>header{font-size:8px;margin-bottom:3px}.widget>header small{display:none}.widget>footer{font-size:6px;flex-direction:column;gap:0}.widget .press-button{gap:2px}.widget .press-button small{font-size:6px}.hat{gap:1px}}
  .viewport-hit{position:absolute;z-index:0;margin:0;padding:0;background:transparent;border:0;border-radius:0;cursor:pointer;touch-action:none}.viewport-hit:focus-visible{outline:2px solid #80bdd8;outline-offset:-3px}.viewport-outline{position:absolute;box-sizing:border-box;border:1px dashed #66849599;pointer-events:none;z-index:4}.viewport-outline.viewport-selected{border:2px solid #86bad4aa;background:#80bdd805}.viewport-outline.viewport-invalid{border-color:#f5a092;background:#e05b4a12}.viewport-label{position:absolute;top:3px;left:3px;max-width:calc(100% - 6px);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;padding:3px 6px;border:1px solid #53768c;border-radius:3px;background:#122837e8;color:#b1ccdc;font:9px ui-monospace,monospace;line-height:1.3;pointer-events:auto;cursor:move;touch-action:none;user-select:none}.viewport-selected .viewport-label{border-color:#86bad4;color:#d2eafa}.viewport-label:focus-visible,.viewport-resize:focus-visible{outline:2px solid #b6e3fa;outline-offset:1px}.viewport-resize{position:absolute;right:-1px;bottom:-1px;width:22px;height:22px;padding:0;background:#23455b;border:1px solid #86bad4;border-radius:5px 0 0 0;color:#cce9f9;font:14px ui-monospace,monospace;pointer-events:auto;cursor:nwse-resize;touch-action:none;user-select:none}.viewport-error{position:absolute;bottom:5px;left:5px;right:5px;width:fit-content;max-width:calc(100% - 10px);z-index:8;padding:5px 7px;border:1px solid #d87566;border-radius:4px;background:#401e1df0;color:#ffd2c9;font:10px ui-monospace,monospace;pointer-events:none}.widget{z-index:2}.widget.selected{z-index:3}.widget.outside-viewport{outline:2px solid #f5a092;outline-offset:1px}
</style>
