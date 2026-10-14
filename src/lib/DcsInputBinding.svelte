<script lang="ts">
  import { dcsTargetKey, physicalDcsControls, dcsInputInterfaces, dcsBindingValue, dcsBindingStatusLabel, type DcsInputBinding, type DcsInputKind, type DcsTarget, type DcsCatalogueControl, type DcsBindingStatus } from './dcs-profile';
  let { binding = $bindable(), targets, catalogue, disabled, status, remove, refresh }: {
    binding: DcsInputBinding; targets: DcsTarget[]; catalogue: DcsCatalogueControl[]; disabled: boolean;
    status?: DcsBindingStatus; remove: () => void; refresh: (gatewayId: string, moduleId: string) => Promise<void>;
  } = $props();
  const target = $derived(targets.find(target => target.gatewayId === binding.gatewayId && target.module.id === binding.moduleId));
  const events = $derived(target?.state?.moduleEvents?.[binding.moduleId]);
  const physical = $derived(physicalDcsControls(binding.kind, events));
  const commands = $derived(catalogue.filter(control => dcsInputInterfaces(binding.kind, control).length));
  const command = $derived(catalogue.find(control => control.id === binding.command));
  const interfaces = $derived(dcsInputInterfaces(binding.kind, command));
  const maximum = $derived(command?.inputs.find(input => input.interface === binding.interface)?.maxValue ?? 65535);
  function chooseTarget(value: string) {
    const [gatewayId, moduleId] = value.split('/');
    binding.gatewayId = gatewayId ?? ''; binding.moduleId = moduleId ?? ''; binding.control = 0;
    if (gatewayId && moduleId) void refresh(gatewayId, moduleId);
  }
  function chooseKind(kind: DcsInputKind) {
    binding.kind = kind; binding.control = physicalDcsControls(kind, events)[0]?.value ?? 0;
    binding.interface = kind === 'encoder' ? 'fixed_step' : 'set_state'; binding.command = '';
  }
  function chooseCommand(id: string) {
    binding.command = id;
    const supported = dcsInputInterfaces(binding.kind, catalogue.find(control => control.id === id));
    binding.interface = (supported[0] ?? (binding.kind === 'encoder' ? 'fixed_step' : 'set_state')) as DcsInputBinding['interface'];
  }
</script>

<article aria-label={`Input binding ${binding.id}`}>
  <header><strong>{binding.id}</strong><span>{dcsBindingStatusLabel(status)}</span><button disabled={disabled} onclick={remove} aria-label={`Remove input ${binding.id}`}>Remove</button></header>
  <fieldset disabled={disabled}>
    <div class="fields">
      <label for={`input-target-${binding.id}`}>Device<select id={`input-target-${binding.id}`} value={dcsTargetKey(binding)} onchange={event => chooseTarget(event.currentTarget.value)}><option value="/">Choose a device</option>{#if binding.gatewayId && !target}<option value={dcsTargetKey(binding)}>Saved device · {binding.moduleId}</option>{/if}{#each targets as item (`${item.gatewayId}/${item.module.id}`)}<option value={`${item.gatewayId}/${item.module.id}`}>{item.label} · {item.gatewayId.slice(6)}{item.module.online ? '' : ' · Offline'}</option>{/each}</select></label>
      <label for={`input-kind-${binding.id}`}>Input type<select id={`input-kind-${binding.id}`} value={binding.kind} onchange={event => chooseKind(event.currentTarget.value as DcsInputKind)}><option value="button">Momentary button</option><option value="switch">Maintained switch</option><option value="encoder">Rotary encoder</option><option value="axis">Calibrated axis</option></select></label>
      <label for={`input-control-${binding.id}`}>Physical control<select id={`input-control-${binding.id}`} bind:value={binding.control}>{#if !physical.some(control => control.value === binding.control)}<option value={binding.control}>Control {binding.control} · refresh capabilities</option>{/if}{#each physical as control (control.value)}<option value={control.value}>{control.label}</option>{/each}</select></label>
      <label for={`input-command-${binding.id}`}>Catalogue command<select id={`input-command-${binding.id}`} value={binding.command} onchange={event => chooseCommand(event.currentTarget.value)}><option value="">Choose a named control</option>{#if binding.command && !commands.some(control => control.id === binding.command)}<option value={binding.command}>{binding.command} · saved selection</option>{/if}{#each commands as control (control.id)}<option value={control.id}>{control.id} · {control.label}</option>{/each}</select></label>
    </div>
    {#if binding.kind === 'encoder'}
      <div class="fields"><label for={`input-interface-${binding.id}`}>Encoder command interface<select id={`input-interface-${binding.id}`} bind:value={binding.interface}>{#if !interfaces.includes(binding.interface)}<option value={binding.interface}>{binding.interface} · choose a catalogue command</option>{/if}{#each interfaces as value (value)}<option {value}>{value === 'fixed_step' ? 'Fixed step · INC / DEC' : 'Variable step · signed delta'}</option>{/each}</select></label>{#if binding.interface === 'variable_step'}<label for={`input-step-${binding.id}`}>Step per detent<input id={`input-step-${binding.id}`} type="number" min="1" max="65535" step="1" bind:value={binding.step} placeholder="1" /></label>{/if}</div>
      <p>Every captured detent sends one ordered step. Old turns are discarded after recovery.</p>
    {:else if binding.kind === 'axis'}
      <label for={`input-deadband-${binding.id}`}>Change threshold in target units<input id={`input-deadband-${binding.id}`} type="number" min="0" max="65535" step="1" bind:value={binding.deadband} placeholder="128" /></label><p>Device calibration supplies 0–65535. Cockpit scales to the command’s range, 0–{maximum}, and sends changes beyond this threshold.</p>
    {:else}
      <div class="fields"><label for={`input-pressed-${binding.id}`}>{binding.kind === 'switch' ? 'Closed value' : 'Pressed value'}<input id={`input-pressed-${binding.id}`} type="number" min="0" max={maximum} step="1" bind:value={binding.pressedValue} placeholder="1" /></label><label for={`input-released-${binding.id}`}>{binding.kind === 'switch' ? 'Open value' : 'Released value'}<input id={`input-released-${binding.id}`} type="number" min="0" max={maximum} step="1" bind:value={binding.releasedValue} placeholder="0" /></label></div><p>{binding.kind === 'button' ? 'A button held during startup or recovery must release before a fresh press.' : 'The current switch state synchronizes at each fresh baseline; later transitions send explicit catalogue states.'}</p>
    {/if}
    {#if !physical.length}<p>Refresh this device’s capabilities. Selector positions are not offered as switch bindings.</p>{/if}
    <button class="refresh" disabled={!target?.module.online} onclick={() => refresh(binding.gatewayId, binding.moduleId)}>Refresh device capabilities</button>
  </fieldset>
  <footer><span>Input readiness: {dcsBindingStatusLabel(status)}</span><span>Last value: {dcsBindingValue(status?.lastValue)}</span><span>Commands: {status?.commandsSent ?? 0}</span></footer>
  {#if status?.error}<p class="error" role="alert">{status.error}</p>{/if}
</article>

<style>
  article { margin: 12px 0; padding: 16px; border: 1px solid var(--line); border-radius: 6px; background: var(--panel); }
  header, footer { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; }
  header strong { flex: 1; color: var(--text); font-size: 12px; }
  header span, footer, p { color: var(--soft); font-size: 11px; line-height: 1.6; }
  footer { margin-top: 12px; }
  fieldset { min-width: 0; padding: 0; border: 0; }
  label { display: block; margin-top: 12px; color: var(--soft); font-size: 11px; }
  input, select { display: block; width: 100%; min-width: 0; margin-top: 5px; padding: 8px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; }
  .fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 12px; }
  button { padding: 6px 9px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; cursor: pointer; }
  button:disabled, fieldset:disabled { opacity: .55; }
  button:disabled { cursor: default; }
  input:focus-visible, select:focus-visible, button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .error { color: var(--amber); }
  @media (max-width: 700px) { .fields { grid-template-columns: 1fr; } }
</style>
