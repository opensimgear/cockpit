<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from '../Icon.svelte';
  import type { DeepReadonly, ProfileDocument, ProfilePortGroup } from '../profiles';
  import type { ObservedResourcePort, ResourceDetails } from '../profile-inventory';
  import type { GameId } from '../games';
  import type { CanvasNode } from '../profile-canvas';
  import { canvasGameId, canvasPortCollapsed, portContractLabel } from '../profile-canvas';
  import type { CompileDiagnostic } from '../profile-compilation';

  let {
    node, document, content, readOnly = false, removeNode, diagnostics = [], openGameSettings,
    companionAuthoring = false, page = null, portsBusy = false, portError = '',
    hasPrevious = false, hasNext = false, loadPorts, togglePorts, selectProviderPort,
    removeProviderPort,
  }: {
    node: CanvasNode; document: DeepReadonly<ProfileDocument>; content?: Snippet<[CanvasNode]>;
    readOnly?: boolean; removeNode?: (id: string) => void;
    diagnostics?: readonly DeepReadonly<CompileDiagnostic>[];
    openGameSettings?: (gameId: GameId, returnState?: unknown) => void;
    companionAuthoring?: boolean; page?: DeepReadonly<ResourceDetails> | null; portsBusy?: boolean; portError?: string;
    hasPrevious?: boolean; hasNext?: boolean; loadPorts?: (op: 'open' | 'next' | 'previous') => void;
    togglePorts?: (category: ProfilePortGroup) => void;
    selectProviderPort?: (nodeId: string, port: DeepReadonly<ObservedResourcePort>) => void;
    removeProviderPort?: (nodeId: string, portId: string) => void;
  } = $props();

  const portCategories: ProfilePortGroup[] = ['outputs', 'inputs', 'actions'];
  const gameId = $derived(canvasGameId(node,document));
  const resourceNode = $derived(node.kind === 'resource' && !!node.resource && node.resource.kind !== 'debug-controller' && node.observation?.resourceType !== 'host-controller');
  const inputs = $derived(node.resource?.ports.filter(port => port.direction === 'input') ?? []);
  const outputs = $derived(node.resource?.ports.filter(port => port.direction === 'output') ?? []);
</script>

<section class="block-inspector" aria-label="Block settings">
  {#if content}{@render content(node)}{/if}



  {#if resourceNode && diagnostics.length}
    <ul class="diagnostics" aria-label="Resource diagnostics">
      {#each diagnostics as diagnostic, index (`compile-${index}`)}<li class:diagnostic-error={diagnostic.severity === 'error'}>{diagnostic.message}</li>{/each}
    </ul>
  {/if}
  {#if portError}<p class="port-error" role="alert">{portError}</p>{/if}

  {#if resourceNode}
    {#if node.resource?.kind!=='game'}
    <details class="ports-section">
      <summary>Ports · {inputs.length + outputs.length}</summary>
      {#each portCategories as category (category)}
        {@const categoryPorts = category === 'outputs' ? outputs : inputs.filter(port => (port.contract.delivery === 'action') === (category === 'actions'))}
        {@const collapsed = canvasPortCollapsed(document, node.id, category)}
        <section class="port-category">
          <button type="button" class="port-category-toggle" aria-expanded={!collapsed} disabled={readOnly} onclick={() => togglePorts?.(category)}>{collapsed ? '▸' : '▾'} {category === 'outputs' ? 'Outputs' : category === 'inputs' ? 'Inputs' : 'Actions'} ({categoryPorts.length})</button>
          {#if !collapsed}
            {#each categoryPorts as port (port.id)}
              <div class="port-row"><strong>{port.name}</strong><code>{port.id}</code><p>{portContractLabel(port.contract)}</p>
                {#if removeProviderPort}<button type="button" class="text-button" aria-label={`Remove ${port.name} port and routes`} disabled={readOnly} onclick={() => removeProviderPort?.(node.id, port.id)}><Icon name="trash" size={14}/>Remove</button>{/if}
              </div>
            {:else}<p class="empty-note">No ports in this group.</p>{/each}
          {/if}
        </section>
      {/each}
    </details>

    {#if node.observation && node.assignment && (!companionAuthoring || node.resource?.kind !== 'companion')}
      <details class="catalogue-section">
        <summary>Available ports · {node.observation.portCount}</summary>
        <button type="button" class="secondary-button" disabled={portsBusy} onclick={() => loadPorts?.('open')}><Icon name="refresh" size={14}/>Refresh</button>
        {#if portsBusy}<p class="status-note" role="status">Loading ports…</p>{/if}
        {#if page}
          <p class="page-count">{page.total ? `${page.offset + 1}–${page.offset + page.ports.length}` : '0'} of {page.total}</p>
          {#each ['output', 'input'] as direction (direction)}
            <details class="direction-section"><summary>{direction === 'output' ? 'Outputs' : 'Inputs'} ({page.ports.filter(port => port.descriptor.direction === direction).length})</summary>
              {#each page.ports.filter(port => port.descriptor.direction === direction) as port (`${port.descriptor.id}/${port.contractHash}`)}
                <div class="port-row"><strong>{port.descriptor.name}</strong><code>{port.descriptor.id}</code><p>{portContractLabel(port.descriptor.contract)}</p>
                  <p class="status-note">{port.current ? 'Current capability' : 'Cached capability'}</p>
                  {#if selectProviderPort}<button type="button" class="secondary-button" disabled={readOnly || portsBusy} onclick={() => selectProviderPort?.(node.id, port)}><Icon name="link" size={14}/>Connect</button>{/if}
                </div>
              {:else}<p class="empty-note">No ports on this page.</p>{/each}
            </details>
          {/each}
          <div class="pagination"><button type="button" class="secondary-button" disabled={portsBusy || !hasPrevious} onclick={() => loadPorts?.('previous')}><Icon name="arrow" size={14}/>Previous</button><button type="button" class="secondary-button" disabled={portsBusy || !hasNext} onclick={() => loadPorts?.('next')}><Icon name="arrow" size={14}/>Next</button></div>
        {/if}
      </details>
    {/if}

    {/if}
    {#if node.observation && node.resource?.kind!=='game'}
      <details class="metadata-section"><summary>Details</summary>
        {#if node.observation.diagnostics.length}<ul class="diagnostics">{#each node.observation.diagnostics as diagnostic,index (index)}<li>{diagnostic}</li>{/each}</ul>{/if}
        {#each node.observation.connections as connection (`${connection.scope}/${connection.key}/${connection.route.id}`)}
          <div class="connection-row"><strong>{connection.route.transport.toUpperCase()}</strong><p>{connection.route.baselineReady ? 'Fresh baseline available' : 'Waiting for a fresh baseline'} · {connection.route.writable ? 'Write capability advertised' : 'Read only'}</p>
            {#if Object.keys(connection.route.attributes).length}<details class="attributes"><summary>Transport attributes</summary><dl>{#each Object.entries(connection.route.attributes) as [key, value] (key)}<div><dt>{key}</dt><dd>{value}</dd></div>{/each}</dl></details>{/if}
          </div>
        {/each}
        {#if Object.keys(node.observation.attributes).length}<details class="attributes"><summary>Resource attributes</summary><dl>{#each Object.entries(node.observation.attributes) as [key, value] (key)}<div><dt>{key}</dt><dd>{value}</dd></div>{/each}</dl></details>{/if}
      </details>
    {/if}
  {/if}

  {#if removeNode||gameId&&openGameSettings}<footer class="controller-actions">{#if gameId&&openGameSettings}<button type="button" class="secondary-button" onclick={()=>openGameSettings?.(gameId!,{profileId:document.id,resourceId:node.id})}><Icon name="arrow" size={14}/>Settings</button>{/if}{#if removeNode}<button type="button" class="secondary-button destructive-button" disabled={readOnly} onclick={() => removeNode?.(node.id)}><Icon name="trash" size={14}/>Remove</button>{/if}</footer>{/if}
</section>

<style>
  .block-inspector{display:flex;flex-direction:column;gap:12px;min-width:0;min-height:100%;box-sizing:border-box;color:var(--text);font-size:var(--ui-text-size)}
  .block-inspector :global(button){font-size:var(--ui-action-size)}
  details{border-top:1px solid var(--line);padding-top:10px}
  summary{color:var(--soft);cursor:pointer;line-height:1.5}
  .port-category{margin-top:10px}
  .port-category-toggle{width:100%;padding:7px 0;border:0;background:transparent;color:var(--soft);text-align:left;cursor:pointer}
  .port-category-toggle:disabled{opacity:.65;cursor:default}
  .port-row{padding:10px 0;border-bottom:1px solid var(--faint-line);overflow-wrap:anywhere}
  .port-row strong{display:block;color:var(--soft)}
  .port-row code{display:block;margin-top:4px;color:var(--muted);font-size:var(--ui-text-size)}
  .port-row p,.connection-row p,.empty-note,.status-note,.page-count{margin:6px 0;color:var(--muted);line-height:1.5}
  .port-row .secondary-button,.port-row .text-button{margin-top:6px}
  .direction-section,.attributes{margin-top:10px}
  .pagination button:first-child :global(svg){transform:rotate(180deg)}
  .pagination{display:flex;justify-content:space-between;gap:10px;margin-top:12px}
  .pagination :global(.secondary-button),.catalogue-section>.secondary-button{display:inline-flex;align-items:center;gap:7px}
  .connection-row{padding:10px 0;border-bottom:1px solid var(--faint-line)}
  dl{margin:8px 0;overflow-wrap:anywhere}.attributes dl>div{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1.5fr);gap:10px;margin:6px 0}dt{color:var(--muted)}dd{margin:0;color:var(--soft)}
  .diagnostics{margin:0;padding-left:20px;color:#c7ad80;line-height:1.55}.diagnostics .diagnostic-error{color:var(--danger,#e67979)}
  .port-error{margin:0;color:var(--danger,#e67979);line-height:1.5}
  .controller-actions{display:flex;justify-content:flex-end;gap:8px;flex-shrink:0;margin-top:auto;padding-top:12px;border-top:1px solid var(--line)}
  .controller-actions>.secondary-button{display:inline-flex;align-items:center;gap:8px}
  .destructive-button{display:inline-flex;align-items:center;gap:8px;border:1px solid var(--danger,#e67979);color:var(--danger,#e67979);background:transparent}
  .destructive-button:hover:enabled{background:color-mix(in srgb,var(--danger,#e67979) 12%,transparent)}
</style>
