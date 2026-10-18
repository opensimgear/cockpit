<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import type { SystemComponent } from './system-inventory';
  import { systemTopology, topologyFit, topologyInitialView, topologyZoom, TOPOLOGY_NODE_HEIGHT, TOPOLOGY_NODE_WIDTH, type TopologyView } from './system-topology';

  let { components, context = components, selectedId, onselect }: {
    components: readonly SystemComponent[];
    context?: readonly SystemComponent[];
    selectedId: string;
    onselect: (id: string) => void;
  } = $props();

  const graph = $derived(systemTopology(components, context));
  const geometry = $derived(graph.nodes.map(node => `${node.component.id}:${node.x}:${node.y}`).join('|') + graph.edges.map(edge => edge.connection.id).join('|'));
  let viewport: HTMLDivElement;
  let width = $state(800), height = $state(560);
  let view = $state<TopologyView>({ x: 24, y: 24, zoom: 1 });
  let dragging = $state(false);
  let drag: { id: number; x: number; y: number; view: TopologyView } | null = null;

  function fit() { view = topologyFit(graph.width, graph.height, width, height); }
  function initialView() { view = topologyInitialView(graph.width, graph.height, width, height); }
  function zoom(multiplier: number, point = { x: width / 2, y: height / 2 }) { view = topologyZoom(view, multiplier, point); }
  function wheel(event: WheelEvent) {
    event.preventDefault();
    const bounds = viewport.getBoundingClientRect();
    zoom(Math.exp(-event.deltaY * 0.0015), { x: event.clientX - bounds.left, y: event.clientY - bounds.top });
  }
  function pointerDown(event: PointerEvent) {
    if (event.button !== 0 || (event.target instanceof Element && event.target.closest('button'))) return;
    viewport.focus({ preventScroll: true });
    viewport.setPointerCapture(event.pointerId);
    drag = { id: event.pointerId, x: event.clientX, y: event.clientY, view: { ...view } };
    dragging = true;
  }
  function pointerMove(event: PointerEvent) {
    if (!drag || drag.id !== event.pointerId) return;
    view = { ...drag.view, x: drag.view.x + event.clientX - drag.x, y: drag.view.y + event.clientY - drag.y };
  }
  function pointerUp(event: PointerEvent) {
    if (drag?.id !== event.pointerId) return;
    drag = null;
    dragging = false;
  }
  function keyboard(event: KeyboardEvent) {
    if (event.target !== viewport) return;
    if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) {
      event.preventDefault();
      view = { ...view, x: view.x + (event.key === 'ArrowLeft' ? 50 : event.key === 'ArrowRight' ? -50 : 0), y: view.y + (event.key === 'ArrowUp' ? 50 : event.key === 'ArrowDown' ? -50 : 0) };
    } else if (event.key === '+' || event.key === '=') { event.preventDefault(); zoom(1.25); }
    else if (event.key === '-') { event.preventDefault(); zoom(0.8); }
    else if (event.key === 'Home' || event.key.toLowerCase() === 'f') { event.preventDefault(); fit(); }
  }
  function reveal(componentId: string) {
    const node = graph.nodes.find(node => node.component.id === componentId);
    if (!node) return;
    const x = node.x * view.zoom + view.x, y = node.y * view.zoom + view.y;
    if (x < 0 || y < 0 || x + TOPOLOGY_NODE_WIDTH * view.zoom > width || y + TOPOLOGY_NODE_HEIGHT * view.zoom > height)
      view = { ...view, x: width / 2 - (node.x + TOPOLOGY_NODE_WIDTH / 2) * view.zoom, y: height / 2 - (node.y + TOPOLOGY_NODE_HEIGHT / 2) * view.zoom };
  }
  function icon(type: SystemComponent['type']) {
    return type === 'host' ? 'monitor' : type === 'gateway' ? 'network' : type === 'companion' ? 'mobile' : type === 'virtual' ? 'command' : type === 'integration' ? 'link' : type === 'controller' ? 'wheel' : 'devices';
  }
  function kind(type: SystemComponent['type']) {
    return type === 'osg' ? 'OSG device' : type === 'controller' ? 'Third-party controller' : type === 'host' ? 'System host' : type[0].toUpperCase() + type.slice(1);
  }
  $effect(() => { geometry; untrack(initialView); });
  onMount(() => {
    const observer = new ResizeObserver(entries => {
      const bounds = entries[0]?.contentRect;
      if (!bounds) return;
      const initial = width === 800 && height === 560;
      width = bounds.width; height = bounds.height;
      if (initial) initialView();
    });
    observer.observe(viewport);
    return () => observer.disconnect();
  });
</script>

<section class="system-topology" aria-label="System topology">
  <div class="topology-heading">
    <div><strong>Current system connections</strong><span>{components.length} components · {graph.edges.length} connections{graph.nodes.some(node => node.context) ? ' · upstream context included' : ''}</span></div>
    <div class="topology-tools" aria-label="Topology view controls">
      <button type="button" onclick={() => zoom(0.8)} aria-label="Zoom out" title="Zoom out">−</button>
      <span aria-live="off">{Math.round(view.zoom * 100)}%</span>
      <button type="button" onclick={() => zoom(1.25)} aria-label="Zoom in" title="Zoom in">+</button>
      <button type="button" onclick={fit} aria-label="Fit all components" title="Fit all components">Fit</button>
    </div>
  </div>
  <div class="topology-legend" aria-label="Connection legend">
    <span><i class="physical"></i>USB / CAN / network</span><span><i class="software"></i>Software relationship</span><span><i class="remembered"></i>Last known connection</span><span><i class="unknown"></i>Unknown transport</span>
  </div>
  <!-- A focusable region provides keyboard panning; its nodes remain native buttons. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div class="topology-viewport" class:dragging bind:this={viewport} role="region" aria-label="Connection graph. Drag background or use arrow keys to pan; scroll or plus and minus to zoom; Home to fit. Tab to select a component." tabindex="0" onwheel={wheel} onpointerdown={pointerDown} onpointermove={pointerMove} onpointerup={pointerUp} onpointercancel={pointerUp} onlostpointercapture={pointerUp} onkeydown={keyboard}>
    <div class="topology-world" style:width={`${graph.width}px`} style:height={`${graph.height}px`} style:transform={`translate(${view.x}px, ${view.y}px) scale(${view.zoom})`}>
      <svg width={graph.width} height={graph.height} aria-hidden="true" class="topology-edges">
        {#each graph.edges as edge (edge.connection.id)}
          <g class:software={edge.connection.transport === 'software'} class:remembered={!edge.connection.current} class:unknown={edge.connection.transport === 'unknown'}>
            <path d={edge.path} />
            <rect x={edge.x - Math.max(34, edge.label.length * 3.1)} y={edge.y - 10} width={Math.max(68, edge.label.length * 6.2)} height="20" rx="4" />
            <text x={edge.x} y={edge.y + 3}>{edge.label}</text>
          </g>
        {/each}
      </svg>
      {#each graph.nodes as node (node.component.id)}
        {@const component = node.component}
        <button type="button" class="topology-node" data-component-id={component.id} class:selected={selectedId === component.id} class:context={node.context} class:offline={component.presence !== 'connected'} style:left={`${node.x}px`} style:top={`${node.y}px`} style:width={`${TOPOLOGY_NODE_WIDTH}px`} style:height={`${TOPOLOGY_NODE_HEIGHT}px`} aria-pressed={selectedId === component.id} aria-label={`${component.name}, ${kind(component.type)}, ${component.presence}${node.context ? ', connection context' : ''}`} onclick={() => onselect(component.id)} onfocus={() => reveal(component.id)} title={`${component.name}\n${component.role}${component.diagnostics.length ? '\n' + component.diagnostics.join('\n') : ''}`}>
          <span class="node-icon"><Icon name={icon(component.type)} size={18} /></span>
          <span class="node-copy"><span class="node-kind">{kind(component.type)}{node.context ? ' · Context' : ''}</span><strong>{component.name}</strong><span class="node-status"><i class:connected={component.presence === 'connected'}></i>{component.presence === 'connected' ? 'Connected' : component.presence === 'offline' ? 'Offline' : component.presence === 'unavailable' ? 'Unavailable' : 'State unknown'}{component.hidden === true ? ' · Hidden' : ''}{component.enabled === false ? ' · Disabled' : ''}</span></span>
          <span class="node-role">{component.role || 'Connection details unknown'}{component.diagnostics.length ? ` · ${component.diagnostics.length} diagnostic${component.diagnostics.length === 1 ? '' : 's'}` : ''}</span>
        </button>
      {/each}
    </div>
    {#if components.length === 0}<p class="topology-empty">No components match the current filters.</p>{/if}
  </div>
  <p class="topology-help">Select any component for details. Drag to pan · Scroll to zoom · Fit to show all. Components without observed connections remain visible.</p>
</section>

<style>
  .system-topology { border: 1px solid var(--line); border-radius: 7px; background: var(--surface); overflow: hidden; }
  .topology-heading { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 16px 18px; border-bottom: 1px solid var(--line); }
  .topology-heading strong { display: block; font-size: 12px; font-weight: 550; }
  .topology-heading div > span { display: block; font-size: 10px; color: var(--muted); margin-top: 6px; }
  .topology-tools { display: flex; align-items: center; gap: 4px; }
  .topology-tools button { padding: 7px 10px; background: var(--subtle); border: 1px solid var(--line); border-radius: 4px; font-size: 12px; color: var(--soft); }
  .topology-tools button:hover { color: var(--accent); border-color: var(--accent); }
  .topology-tools > span { min-width: 42px; text-align: center; font-size: 10px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .topology-legend { display: flex; flex-wrap: wrap; gap: 12px 18px; padding: 12px 18px; color: var(--muted); font-size: 9px; border-bottom: 1px solid var(--faint-line); }
  .topology-legend span { display: flex; align-items: center; gap: 6px; }
  .topology-legend i { display: inline-block; width: 20px; height: 0; border-top: 2px solid var(--accent); }
  .topology-legend .software { border-color: #8fb6e9; border-top-style: dotted; }
  .topology-legend .remembered { border-color: var(--muted); border-top-style: dashed; }
  .topology-legend .unknown { border-color: #d7ae77; border-top-style: dotted; }
  .topology-viewport { position: relative; height: clamp(420px, 64vh, 760px); min-height: 420px; overflow: hidden; background: var(--bg); background-image: radial-gradient(var(--line) .7px, transparent .7px); background-size: 18px 18px; cursor: grab; touch-action: none; }
  .topology-viewport.dragging { cursor: grabbing; user-select: none; }
  .topology-viewport:focus-visible { outline: 2px solid var(--accent); outline-offset: -3px; }
  .topology-world { position: absolute; top: 0; left: 0; transform-origin: 0 0; }
  .topology-edges { position: absolute; overflow: visible; pointer-events: none; }
  .topology-edges path { stroke: var(--accent); stroke-width: 1.5; fill: none; }
  .topology-edges rect { fill: var(--bg); stroke: var(--line); stroke-width: .6; }
  .topology-edges text { fill: var(--soft); font-size: 10px; text-anchor: middle; }
  .topology-edges .software path { stroke: #8fb6e9; stroke-dasharray: 3 4; }
  .topology-edges .unknown path { stroke: #d7ae77; stroke-dasharray: 2 4; }
  .topology-edges .remembered path { stroke: var(--muted); stroke-dasharray: 7 5; }
  .topology-edges .remembered text { fill: var(--muted); }
  .topology-node { position: absolute; display: flex; flex-wrap: wrap; align-content: center; align-items: start; gap: 10px; padding: 14px; border: 1px solid var(--line); border-radius: 7px; background: var(--surface); color: var(--text); text-align: left; box-shadow: 0 3px 12px var(--shadow); }
  .topology-node:hover { border-color: var(--muted); }
  .topology-node.selected { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }
  .topology-node.context { border-style: dashed; }
  .topology-node.offline .node-icon { color: var(--muted); }
  .node-icon { display: grid; place-items: center; width: 30px; height: 32px; background: var(--subtle); color: var(--accent); border-radius: 5px; flex-shrink: 0; }
  .node-copy { min-width: 0; flex: 1; }
  .node-kind { display: block; font-size: 8px; color: var(--muted); margin-bottom: 5px; }
  .node-copy strong { font-size: 12px; font-weight: 550; display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .node-status { display: flex; align-items: center; gap: 5px; font-size: 9px; color: var(--muted); margin-top: 7px; white-space: nowrap; }
  .node-status i { height: 5px; width: 5px; border-radius: 50%; background: var(--muted); }
  .node-status i.connected { background: var(--accent); }
  .node-role { flex-basis: 100%; font-size: 9px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; border-top: 1px solid var(--faint-line); padding-top: 9px; }
  .topology-help { padding: 12px 18px; font-size: 9px; line-height: 1.6; color: var(--muted); border-top: 1px solid var(--line); }
  .topology-empty { position: absolute; left: 20px; right: 20px; top: 35px; text-align: center; font-size: 12px; color: var(--muted); pointer-events: none; }
  @media (max-width: 680px) { .topology-heading { align-items: start; flex-direction: column; } .topology-viewport { min-height: 360px; height: 55vh; } }
</style>
