<script lang="ts">
  import Notification from './Notification.svelte';
  import { dcsTargetKey, dcsOutputActions, dcsEndpointLabel, dcsBindingValue, dcsOutputIndex, dcsBindingStatusLabel, type DcsOutputBinding, type DcsOutputFormat, type DcsTarget, type DcsCatalogueControl, type DcsBindingStatus } from './dcs-profile';
  let { binding = $bindable(), targets, catalogue, disabled, status, remove, refresh }: {
    binding: DcsOutputBinding; targets: DcsTarget[]; catalogue: DcsCatalogueControl[]; disabled: boolean;
    status?: DcsBindingStatus; remove: () => void; refresh: (gatewayId: string, moduleId: string) => Promise<void>;
  } = $props();
  const target = $derived(targets.find(target => target.gatewayId === binding.gatewayId && target.module.id === binding.moduleId));
  const actions = $derived(dcsOutputActions(binding.format.kind, target?.state?.moduleActions?.[binding.moduleId]));
  const action = $derived(actions.find(action => action.id === binding.actionId));
  const sourceType = $derived(binding.format.kind === 'text' ? 'string' : 'integer');
  const controls = $derived(catalogue.filter(control => control.outputs.some(output => output.type === sourceType)));
  const control = $derived(catalogue.find(control => control.id === binding.control));
  function chooseTarget(value: string) {
    const [gatewayId, moduleId] = value.split('/'); binding.gatewayId = gatewayId ?? ''; binding.moduleId = moduleId ?? ''; binding.actionId = '';
    if (gatewayId && moduleId) void refresh(gatewayId, moduleId);
  }
  function chooseFormat(kind: DcsOutputFormat['kind']) { binding.format.kind = kind; binding.control = ''; binding.output = 0; binding.actionId = ''; }
  function chooseControl(id: string) { binding.control = id; binding.output = dcsOutputIndex(binding.format.kind, catalogue.find(control => control.id === id)); }
</script>

<article aria-label={`Output binding ${binding.id}`}>
  <header><strong>{binding.id}</strong><span>{dcsBindingStatusLabel(status)}</span><button disabled={disabled} onclick={remove} aria-label={`Remove output ${binding.id}`}>Remove</button></header>
  <fieldset disabled={disabled}>
    <div class="fields">
      <label for={`output-target-${binding.id}`}>Device<select id={`output-target-${binding.id}`} value={dcsTargetKey(binding)} onchange={event => chooseTarget(event.currentTarget.value)}><option value="/">Choose a device</option>{#if binding.gatewayId && !target}<option value={dcsTargetKey(binding)}>Saved device · {binding.moduleId}</option>{/if}{#each targets as item (`${item.gatewayId}/${item.module.id}`)}<option value={`${item.gatewayId}/${item.module.id}`}>{item.label} · {item.gatewayId.replace(/^[^:]+:/, '')}{item.module.online ? '' : ' · Offline'}</option>{/each}</select></label>
      <label for={`output-format-${binding.id}`}>Feedback format<select id={`output-format-${binding.id}`} value={binding.format.kind} onchange={event => chooseFormat(event.currentTarget.value as DcsOutputFormat['kind'])}><option value="boolean">On/off LED</option><option value="text">Text display</option><option value="integer">Rounded number display</option><option value="fixed">Fixed decimal display</option></select></label>
      <label for={`output-control-${binding.id}`}>Catalogue feedback<select id={`output-control-${binding.id}`} value={binding.control} onchange={event => chooseControl(event.currentTarget.value)}><option value="">Choose a named control</option>{#if binding.control && !controls.some(control => control.id === binding.control)}<option value={binding.control}>{binding.control} · saved selection</option>{/if}{#each controls as control (control.id)}<option value={control.id}>{control.id} · {control.label}</option>{/each}</select></label>
      <label for={`output-action-${binding.id}`}>Device output<select id={`output-action-${binding.id}`} bind:value={binding.actionId}><option value="">Choose an advertised action</option>{#if binding.actionId && !actions.some(action => action.id === binding.actionId)}<option value={binding.actionId}>{binding.actionId} · refresh capabilities</option>{/if}{#each actions as action (action.id)}<option value={action.id}>{action.label} · {action.endpoint}</option>{/each}</select></label>
    </div>
    {#if (control?.outputs.length ?? 0) > 1}<label for={`output-index-${binding.id}`}>Catalogue output<select id={`output-index-${binding.id}`} bind:value={binding.output}>{#each control?.outputs ?? [] as output (output.index)}{#if output.type === sourceType}<option value={output.index}>Output {output.index} · {output.type}{output.maxLength ? ` · ${output.maxLength} bytes` : ''}</option>{/if}{/each}</select></label>{/if}
    <p>{dcsEndpointLabel(action)}</p>
    {#if binding.format.kind !== 'boolean'}
      {#if binding.format.kind !== 'text'}<div class="fields"><label for={`output-scale-${binding.id}`}>Scale<input id={`output-scale-${binding.id}`} type="number" min="-1000000" max="1000000" step="any" bind:value={binding.format.scale} placeholder="1" /></label>{#if binding.format.kind === 'fixed'}<label for={`output-decimals-${binding.id}`}>Decimal places<input id={`output-decimals-${binding.id}`} type="number" min="0" max="6" step="1" bind:value={binding.format.decimals} placeholder="0" /></label>{/if}</div>{/if}
      <div class="fields"><label for={`output-prefix-${binding.id}`}>Prefix<input id={`output-prefix-${binding.id}`} type="text" bind:value={binding.format.prefix} placeholder="Optional" /></label><label for={`output-suffix-${binding.id}`}>Suffix<input id={`output-suffix-${binding.id}`} type="text" bind:value={binding.format.suffix} placeholder="Optional" /></label><label for={`output-align-${binding.id}`}>Alignment<select id={`output-align-${binding.id}`} value={binding.format.align ?? 'left'} onchange={event => binding.format.align = event.currentTarget.value as 'left' | 'right'}><option value="left">Left</option><option value="right">Right</option></select></label><label for={`output-overflow-${binding.id}`}>When text exceeds the output<select id={`output-overflow-${binding.id}`} value={binding.format.overflow ?? 'truncate'} onchange={event => binding.format.overflow = event.currentTarget.value as 'truncate' | 'reject'}><option value="truncate">Truncate whole characters</option><option value="reject">Reject and report an error</option></select></label><label for={`output-unknown-${binding.id}`}>Unknown feedback text<input id={`output-unknown-${binding.id}`} type="text" bind:value={binding.format.unknownText} placeholder="Blank" /></label><label for={`output-stale-${binding.id}`}>Stale feedback text<input id={`output-stale-${binding.id}`} type="text" bind:value={binding.format.staleText} placeholder="Blank" /></label></div>
      <p>Formatting respects the advertised cells, encoding and byte limit. Updates use complete DCS frames and coalesce to the newest value.</p>
    {:else}<p>Unknown or stale feedback requests Off. This profile manages the selected action while running.</p>{/if}
    <button disabled={!target?.module.online} onclick={() => refresh(binding.gatewayId, binding.moduleId)}>Refresh device capabilities</button>
  </fieldset>
  <footer><span>Desired: {dcsBindingValue(status?.desired)}</span><span>Last applied: {dcsBindingValue(status?.applied)}</span></footer>
  {#if status?.error}<Notification message={String(status.error)} kind="error" />{/if}
</article>

<style>
  article{ margin: 12px 0; padding: 16px; border: 1px solid var(--line); border-radius: 6px; background: var(--panel); }header,footer{ display: flex; flex-wrap: wrap; align-items: center; gap: 12px; }header strong{ flex: 1; color: var(--text); font-size: 12px; }header span,footer,p{ color: var(--soft); font-size: 11px; line-height: 1.6; overflow-wrap: anywhere; }footer{ margin-top: 12px; }fieldset{ min-width: 0; padding: 0; border: 0; }label{ display: block; margin-top: 12px; color: var(--soft); font-size: 11px; }input,select{ display: block; width: 100%; min-width: 0; margin-top: 5px; padding: 8px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; }.fields{ display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 12px; }button{ padding: 6px 9px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; cursor: pointer; }button:disabled,fieldset:disabled{ opacity: .55; }button:disabled{ cursor: default; }input:focus-visible,select:focus-visible,button:focus-visible{ outline: 2px solid var(--accent); outline-offset: 2px; }
  @media (max-width: 700px) {.fields{ grid-template-columns: 1fr; } }
</style>
