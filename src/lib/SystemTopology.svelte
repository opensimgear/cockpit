<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import { systemDeviceIcon } from './system-device-icons';
  import { systemDeviceSubtext, systemStateLabel, type SystemComponent } from './system-inventory';
  import { systemTopology, topologyFit, topologyInitialView, topologyZoom, TOPOLOGY_NODE_WIDTH, type TopologyView } from './system-topology';

  let { components, context = components, selectedId, onselect }: {
    components: readonly SystemComponent[];
    context?: readonly SystemComponent[];
    selectedId: string;
    onselect: (id: string) => void;
  } = $props();

  const graph = $derived(systemTopology(components, context));
  const ports = $derived([...new Map([...graph.edges.map(edge => [
    `${edge.end.x}:${edge.end.y}:${edge.connection.transport}`, { ...edge.end, label: edge.label, transport: edge.connection.transport },
  ] as const), ...graph.rows.filter(row => row.source).map(row => [
    `${row.source!.x}:${row.source!.y}:${row.transport}`, { ...row.source!, transport: row.transport, label: row.transport === 'software' ? 'Virtual' : row.transport === 'unknown' ? 'Unknown transport' : row.transport.toUpperCase() },
  ] as const)]).values()]);
  const protocolIcons = { usb: 'usb', can: 'chip', network: 'network', bluetooth: 'bluetooth', software: 'command', unknown: 'help' };
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
    const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? height : 1;
    if (!event.ctrlKey && !event.metaKey) {
      view = { ...view, x: view.x - event.deltaX * unit, y: view.y - event.deltaY * unit };
      return;
    }
    const bounds = viewport.getBoundingClientRect();
    zoom(Math.exp(-event.deltaY * unit * 0.0015), { x: event.clientX - bounds.left, y: event.clientY - bounds.top });
  }
  function pointerDown(event: PointerEvent) {
    if (event.button !== 0 || (event.target instanceof Element && event.target.closest('button, .connection-port'))) return;
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
    if (x < 0 || y < 0 || x + TOPOLOGY_NODE_WIDTH * view.zoom > width || y + node.height * view.zoom > height)
      view = { ...view, x: width / 2 - (node.x + TOPOLOGY_NODE_WIDTH / 2) * view.zoom, y: height / 2 - (node.y + node.height / 2) * view.zoom };
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
    <div><strong>Current system connections</strong><span>{components.length} components · {graph.connectionCount} current connections{graph.nodes.some(node => node.context) ? ' · upstream context included' : ''}</span></div>
    <div class="topology-tools" aria-label="Topology view controls">
      <button class="topology-fit" type="button" onclick={fit} aria-label="Fit all components" title="Fit all components"><Icon name="fit" size={16}/></button>
      <div class="topology-zoom">
        <button type="button" onclick={() => zoom(0.8)} aria-label="Zoom out" title="Zoom out">−</button>
        <output aria-label="Topology zoom">{Math.round(view.zoom * 100)}%</output>
        <button type="button" onclick={() => zoom(1.25)} aria-label="Zoom in" title="Zoom in">+</button>
      </div>
    </div>
  </div>
  <div class="topology-legend" aria-label="Connection legend">
    <span><i class="physical"></i>Current connection</span><span><i class="software"></i>Virtual connection</span><span>Hover a connector to see its protocol</span>
  </div>
  <!-- A focusable region provides keyboard panning; its nodes remain native buttons. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div class="topology-viewport" class:dragging bind:this={viewport} role="region" aria-label="Connection graph. Scroll, drag background or use arrow keys to pan; Ctrl or Cmd plus scroll, or plus and minus keys to zoom; Home to fit. Tab to select a component." tabindex="0" onwheel={wheel} onpointerdown={pointerDown} onpointermove={pointerMove} onpointerup={pointerUp} onpointercancel={pointerUp} onlostpointercapture={pointerUp} onkeydown={keyboard}>
    <div class="topology-world" style:width={`${graph.width}px`} style:height={`${graph.height}px`} style:transform={`translate(${view.x}px, ${view.y}px) scale(${view.zoom})`}>
      {#if graph.offline}<div class="offline-zone" style:left={`${graph.offline.x}px`} style:top={`${graph.offline.y}px`} style:width={`${graph.offline.width}px`} style:height={`${graph.offline.height}px`}><strong>Disconnected <span>{graph.offline.count}</span></strong></div>{/if}
      {#each graph.rows as row (row.id)}<div class="bus-label" data-bus-id={row.id} title={row.label} style:left={`${row.x + 48}px`} style:top={`${row.y}px`} style:width={`${row.width - 48}px`}><Icon name={protocolIcons[row.transport]} size={14} /><strong>{row.label}</strong><span>{row.count} {row.count === 1 ? 'device' : 'devices'}</span></div>{/each}
      <svg width={graph.width} height={graph.height} aria-hidden="true" class="topology-edges">
        <path class="host-trunk" d={graph.trunk} />
        {#each graph.rows.filter(row => row.path) as row (row.id)}<g data-bus-row={row.id} class:software={row.transport === 'software'}><path d={row.anchor} /><path d={row.path} /></g>{/each}
        {#each graph.edges as edge (edge.connection.id)}
          <g data-from={edge.connection.from} data-to={edge.connection.to} class:software={edge.connection.transport === 'software'} class:unknown={edge.connection.transport === 'unknown'}>
            <path d={edge.path} />
          </g>
        {/each}
      </svg>
      {#each ports as port (`${port.x}:${port.y}:${port.transport}`)}<span class="connection-port" role="img" aria-label={`${port.label} connector`} title={port.label} style:left={`${port.x}px`} style:top={`${port.y}px`}><Icon name={protocolIcons[port.transport]} size={12} /><span class="port-tooltip" aria-hidden="true">{port.label}</span></span>{/each}
      {#each graph.nodes as node (node.component.id)}
        {@const component = node.component}
        {@const state = systemStateLabel(component)}
        <button type="button" class="topology-node" data-component-id={component.id} class:selected={selectedId === component.id} class:context={node.context} class:offline={component.presence !== 'connected'} style:left={`${node.x}px`} style:top={`${node.y}px`} style:width={`${TOPOLOGY_NODE_WIDTH}px`} style:height={`${node.height}px`} aria-pressed={selectedId === component.id} aria-label={`${component.name}, ${kind(component.type)}, ${state}${node.context ? ', connection context' : ''}`} onclick={() => onselect(component.id)} onfocus={event => { if (event.currentTarget.matches(':focus-visible')) reveal(component.id); }} title={`${component.name}\n${component.role}\n${state}${component.diagnostics.length ? '\n' + component.diagnostics.join('\n') : ''}`}>
          <span class="node-icon"><Icon name={component.type === 'companion' ? 'mobile' : systemDeviceIcon(component)} size={18} /></span>
          <span class="node-copy"><span class="node-kind">{kind(component.type)}{node.context ? ' · Context' : ''}</span><strong>{component.name}</strong><span class="node-status" title={state}><span>{systemDeviceSubtext(component)}</span></span></span>
          <span class="node-role">{component.role || 'Connection details unknown'}{node.alternateConnections ? ` · +${node.alternateConnections} other route${node.alternateConnections === 1 ? '' : 's'}` : ''}{component.diagnostics.length ? ` · ${component.diagnostics.length} diagnostic${component.diagnostics.length === 1 ? '' : 's'}` : ''}</span>
        </button>
      {/each}
    </div>
    {#if components.length === 0}<p class="topology-empty">No components match the current filters.</p>{/if}
  </div>
  <p class="topology-help">One row per protocol; gateways have their own device rows. Select a device to inspect all routes. Scroll or drag to pan · Ctrl/Cmd + scroll to zoom · Fit to show all. Disconnected devices remain unwired on the right.</p>
</section>

<style>
  .system-topology { border: 1px solid var(--line); border-radius: 7px; background: var(--surface); overflow: hidden; }
  .topology-heading { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 16px 18px; border-bottom: 1px solid var(--line); }
  .topology-heading strong { display: block; font-size: 12px; font-weight: 550; }
  .topology-heading div > span { display: block; font-size: 10px; color: var(--muted); margin-top: 6px; }
  .topology-tools { display: flex; align-items: center; gap: 8px; }
  .topology-fit { display:grid;place-items:center;flex-shrink:0;width:32px;height:32px;padding:0;border:1px solid var(--line);border-radius:6px;background:transparent;color:var(--soft);cursor:pointer; }
  .topology-fit:hover { background:var(--surface-raised);color:var(--text); }
  .topology-tools button:focus-visible { outline:2px solid var(--accent);outline-offset:2px; }
  .topology-zoom { display:flex;align-items:center;border:1px solid var(--line);border-radius:6px; }
  .topology-zoom button { width:30px;height:30px;padding:0;border:0;background:transparent;color:var(--soft);font-size:16px;cursor:pointer; }
  .topology-zoom output { min-width:44px;text-align:center;font-size:10px;color:var(--muted);font-variant-numeric:tabular-nums; }
  .topology-legend { display: flex; flex-wrap: wrap; gap: 12px 18px; padding: 12px 18px; color: var(--muted); font-size: 9px; border-bottom: 1px solid var(--faint-line); }
  .topology-legend span { display: flex; align-items: center; gap: 6px; }
  .topology-legend i { display: inline-block; width: 20px; height: 0; border-top: 2px solid var(--accent); }
  .topology-legend .software { border-color: #8fb6e9; border-top-style: dotted; }
  .topology-viewport { position: relative; height: clamp(420px, 64vh, 760px); min-height: 420px; overflow: hidden; background: var(--bg); background-image: radial-gradient(var(--line) 1px, transparent 1px); background-size: 18px 18px; cursor: grab; touch-action: none; }
  .topology-viewport.dragging { cursor: grabbing; user-select: none; }
  .topology-viewport:focus-visible { outline: 2px solid var(--accent); outline-offset: -3px; }
  .topology-world { position: absolute; top: 0; left: 0; transform-origin: 0 0; }
  .bus-label {position:absolute;display:flex;align-items:center;gap:9px;height:24px;color:var(--soft);white-space:nowrap;overflow:hidden}.bus-label strong {font-size:12px;font-weight:550;overflow:hidden;text-overflow:ellipsis}.bus-label > span {font-size:10px;color:var(--muted);flex-shrink:0}
  .topology-edges { position: absolute; overflow: visible; pointer-events: none; }
  .topology-edges path { stroke: var(--muted); stroke-opacity: .55; stroke-width: 1.25; fill: none; stroke-linejoin: round; }
  .topology-edges .software path { stroke-dasharray: 3 4; }
  .topology-edges .unknown path { stroke: #d7ae77; stroke-dasharray: 2 4; }
  .offline-zone {position:absolute;border:1px dashed var(--line);border-radius:12px;background:var(--subtle)}
  .offline-zone > strong {display:flex;align-items:center;gap:8px;margin:16px;font-size:12px;font-weight:500;color:var(--soft)}.offline-zone > strong span {font-size:10px;color:var(--muted)}
  .connection-port {position:absolute;z-index:3;display:grid;place-items:center;width:20px;height:20px;transform:translate(-50%,-50%);border:1px solid var(--line);border-radius:4px;background:var(--surface-raised);color:var(--soft);cursor:help}
  .connection-port:hover {border-color:var(--accent);color:var(--accent);z-index:4}.port-tooltip {display:none;position:absolute;bottom:calc(100% + 8px);left:50%;transform:translateX(-50%);padding:6px 9px;border:1px solid var(--line);border-radius:5px;background:var(--surface-raised);color:var(--text);white-space:nowrap;font-size:11px;pointer-events:none}.connection-port:hover .port-tooltip {display:block}
  .topology-node { position: absolute; display: flex; flex-wrap: wrap; align-content: center; align-items: start; gap: 10px; padding: 14px; border: 1px solid var(--line); border-radius: 7px; background: var(--surface); color: var(--text); text-align: left; box-shadow: 0 3px 12px var(--shadow); }
  .topology-node:hover { border-color: var(--muted); }
  .topology-node.selected { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }
  .topology-node.context { border-style: dashed; }
  .topology-node.offline .node-icon { color: var(--muted); background: transparent; }
  .topology-node.offline .node-role {display:none}
  .node-icon { display: grid; place-items: center; width: 30px; height: 32px; background: var(--accent-bg); color: var(--accent); border-radius: 5px; flex-shrink: 0; }
  .node-copy { min-width: 0; flex: 1; }
  .node-kind { display: block; font-size: 8px; color: var(--muted); margin-bottom: 5px; }
  .node-copy strong { font-size: 12px; font-weight: 550; display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .node-status { display: flex; align-items: start; gap: 5px; font-size: 9px; line-height: 1.25; color: var(--muted); margin-top: 7px; }
  .node-status > span { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; }
  .node-role { flex-basis: 100%; font-size: 9px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; border-top: 1px solid var(--faint-line); padding-top: 9px; }
  .topology-help { padding: 12px 18px; font-size: 9px; line-height: 1.6; color: var(--muted); border-top: 1px solid var(--line); }
  .topology-empty { position: absolute; left: 20px; right: 20px; top: 35px; text-align: center; font-size: 12px; color: var(--muted); pointer-events: none; }
  @media (max-width: 680px) { .topology-heading { align-items: start; flex-direction: column; } .topology-viewport { min-height: 360px; height: 55vh; } }
</style>
