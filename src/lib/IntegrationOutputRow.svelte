<script lang="ts">
  import Notification from './Notification.svelte';
  import ActionControl from './ActionControl.svelte';
  import { dcsBindingValue, dcsEndpointLabel } from './dcs-profile';
  import { integrationChannels, integrationOutputActions, roleTarget, type IntegrationConfig, type IntegrationOutput, type IntegrationCatalogue, type IntegrationTarget, type IntegrationOutputStatus, type IntegrationRequest, type IntegrationFormat } from './integration-profile';
  let { binding = $bindable(), config, catalogue, targets, disabled, running, busy, status, remove, refresh, request }: {
    binding: IntegrationOutput; config: IntegrationConfig; catalogue?: IntegrationCatalogue; targets: IntegrationTarget[];
    disabled: boolean; running: boolean; busy: boolean; status?: IntegrationOutputStatus;
    remove: () => void; refresh: (gatewayId: string, moduleId: string) => Promise<unknown>;
    request: (request: IntegrationRequest) => Promise<unknown>;
  } = $props();
  let overrideMs = $state(10000), notice = $state('');
  const target = $derived(roleTarget(config,binding.role,targets));
  const snapshot = $derived(target?.state?.moduleActions?.[target.module.id]);
  const actions = $derived(integrationOutputActions(binding.format.kind,snapshot));
  const action = $derived(actions.find(a => a.id === binding.actionId && a.endpoint === binding.endpoint));
  const channels = $derived(integrationChannels(binding.format.kind,catalogue));
  const channelValue = $derived(`${binding.channel}/${binding.output ?? 0}`);
  const ready = $derived(!!target?.state?.connected && !!target.module.online && !target.busy && !target.state.reconnecting && !target.state.layoutPending && snapshot?.boot === target.module.boot);
  function chooseFormat(kind: IntegrationFormat['kind']) {
    binding.format = kind === 'rgb' ? {kind,bands:[{minimum:0.7,color:'00FF00'},{minimum:0.9,color:'FF0000'}],unknownColor:'000000',staleColor:'000000'} :
      kind === 'threshold' ? {kind,threshold:0.9,comparison:'gte'} : {kind,overflow:'truncate'};
    binding.channel = ''; binding.output = 0; binding.actionId = ''; binding.endpoint = '';
  }
  function chooseChannel(value: string) {
    const channel = channels.find(c => `${c.id}/${c.output ?? 0}` === value);
    binding.channel = channel?.id ?? ''; binding.output = channel?.output ?? 0;
  }
  function chooseAction(value: string) {
    const selected = actions.find(a => a.id === value);
    binding.actionId = selected?.id ?? ''; binding.endpoint = selected?.endpoint ?? '';
  }
  async function override(values: (boolean | number | string)[]) {
    if (!Number.isInteger(overrideMs) || overrideMs < 1 || overrideMs > 30000) { notice = 'Override duration must be from 1 to 30000 ms.'; return false; }
    notice = ''; return !!await request({command:'integration_override',bindingId:binding.id,values,durationMs:overrideMs});
  }
</script>

<article aria-label={`Output binding ${binding.id}`}>
  <fieldset {disabled}><legend>Output {binding.id}</legend>
    <div class="fields">
      <label>Binding identifier<input bind:value={binding.id} maxlength="31" spellcheck="false" /></label>
      <label>Device role<select bind:value={binding.role}><option value="">Choose role</option>{#each config.document.roles as role (role.id)}<option value={role.id}>{role.label}</option>{/each}</select></label>
      <label>Format<select value={binding.format.kind} onchange={event => chooseFormat(event.currentTarget.value as IntegrationFormat['kind'])}>{#each ['boolean','threshold','rgb','text','integer','fixed'] as kind (kind)}<option value={kind}>{kind === 'threshold' ? 'Numeric threshold → LED' : kind === 'rgb' ? 'Numeric bands → RGB' : kind}</option>{/each}</select></label>
      <label>Named telemetry channel<select value={channelValue} onchange={event => chooseChannel(event.currentTarget.value)}><option value="">Choose channel</option>{#each channels as channel (`${channel.id}/${channel.output ?? 0}`)}<option value={`${channel.id}/${channel.output ?? 0}`}>{channel.label} · {channel.id}{channel.output !== undefined ? ` · output ${channel.output}` : ''}{channel.unit ? ` · ${channel.unit}` : ''}</option>{/each}</select></label>
      <label>Advertised device action<select value={binding.actionId} onchange={event => chooseAction(event.currentTarget.value)}><option value="">Choose compatible action</option>{#each actions as option (option.id)}<option value={option.id}>{option.label} · {option.endpoint}</option>{/each}</select></label>
    </div>
    <p>{action?.parameters[0].type === 'rgb' ? 'Readable RGB output · six hexadecimal digits' : dcsEndpointLabel(action)}</p>
    {#if binding.format.kind === 'threshold'}<div class="fields"><label>Threshold<input type="number" step="any" bind:value={binding.format.threshold} /></label><label>Comparison<select bind:value={binding.format.comparison}><option value="gte">At or above</option><option value="lte">At or below</option></select></label></div>{/if}
    {#if binding.format.kind === 'rgb'}
      <div class="bands">{#each binding.format.bands ?? [] as band, index (index)}<div><label>Band {index + 1} minimum<input type="number" step="any" bind:value={band.minimum} /></label><label>RGB hex<input bind:value={band.color} maxlength="6" spellcheck="false" /></label><button onclick={() => binding.format.bands!.splice(index,1)}>Remove band</button></div>{/each}</div>
      <button disabled={disabled || (binding.format.bands?.length ?? 0) >= 8} onclick={() => binding.format.bands?.push({minimum:(binding.format.bands.at(-1)?.minimum ?? 0)+0.1,color:'FFFFFF'})}>Add RGB band</button>
      <div class="fields"><label>Unknown RGB<input bind:value={binding.format.unknownColor} maxlength="6" /></label><label>Stale RGB<input bind:value={binding.format.staleColor} maxlength="6" /></label></div>
    {/if}
    {#if ['text','integer','fixed'].includes(binding.format.kind)}
      <div class="fields">
        {#if binding.format.kind !== 'text'}<label>Scale<input type="number" step="any" bind:value={binding.format.scale} placeholder="1" /></label><label>Decimal places<input type="number" min="0" max="6" step="1" bind:value={binding.format.decimals} placeholder="0" /></label>{/if}
        <label>Prefix<input bind:value={binding.format.prefix} /></label><label>Suffix<input bind:value={binding.format.suffix} /></label>
        <label>Alignment<select bind:value={binding.format.align}><option value="left">Left</option><option value="right">Right</option></select></label><label>Overflow<select bind:value={binding.format.overflow}><option value="truncate">Truncate whole characters</option><option value="reject">Reject</option></select></label>
        <label>Unknown text<input bind:value={binding.format.unknownText} /></label><label>Stale text<input bind:value={binding.format.staleText} /></label>
      </div>
    {/if}
    <div class="tools"><button disabled={disabled || !target} onclick={() => target && refresh(target.gatewayId,target.module.id)}>Refresh actions</button><button {disabled} onclick={remove}>Remove binding</button></div>
  </fieldset>
  <p>Desired: {dcsBindingValue(status?.desired)} · Applied: {dcsBindingValue(status?.applied)} · {status?.owned ? 'Owned by this integration' : running ? 'Waiting for ownership' : 'Stopped'}{status?.overrideRemainingMs ? ` · override ${status.overrideRemainingMs} ms remaining` : ''}</p>
  {#if status?.error}<Notification message={String(status.error)} kind="error" />{/if}
  {#if running && status?.owned && action && snapshot}
    <details><summary>Override temporarily</summary><p>The integration keeps its output claim. Expiry resumes fresh telemetry; feedback loss and Stop apply the fallback.</p><label>Override duration (ms)<input type="number" min="1" max="30000" step="1" bind:value={overrideMs} /></label>
      <ActionControl {action} {snapshot} {ready} {busy} invoke={(_action,values) => override(values)} />
      <button disabled={busy} onclick={() => request({command:'integration_override',bindingId:binding.id,resume:true})}>Resume telemetry now</button>
    </details>
  {/if}
  {#if notice}<Notification message={String(notice)} kind="error" />{/if}
</article>

<style>
  article{ margin: 12px 0; padding: 14px; border: 1px solid var(--line); border-radius: 6px; }fieldset{ border: 0; margin: 0; padding: 0; min-width: 0; }legend{ font-size: 12px; font-weight: 600; }.fields{ display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 10px; }label{ display: block; margin-top: 8px; color: var(--soft); font-size: 11px; }input,select{ display: block; width: 100%; min-width: 0; margin-top: 5px; padding: 7px; color: var(--text); background: var(--input-bg); border: 1px solid var(--line); border-radius: 4px; }p,summary{ font-size: 11px; line-height: 1.7; color: var(--soft); }.tools,.bands > div{ display: flex; gap: 10px; align-items: end; flex-wrap: wrap; margin: 12px 0; }.bands label{ flex: 1; }button{ border: 1px solid var(--line); border-radius: 4px; padding: 7px 10px; background: var(--input-bg); color: var(--text); cursor: pointer; font-size: 11px; }:disabled{ opacity: .55; cursor: default; }:focus-visible{ outline: 2px solid var(--accent); outline-offset: 2px; }details{ margin-top: 12px; }summary{ cursor: pointer; }
  @media (max-width: 650px) {.fields{ grid-template-columns: 1fr; } }
</style>
