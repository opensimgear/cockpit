<script lang="ts">
  import { integrationPhysicalControls, normalizeIntegrationCatalogue, roleTarget, type IntegrationConfig, type IntegrationInput, type IntegrationCatalogue, type IntegrationTarget } from './integration-profile';
  let { binding = $bindable(), config, catalogue, targets, disabled, remove, refresh }: {
    binding: IntegrationInput; config: IntegrationConfig; catalogue?: IntegrationCatalogue; targets: IntegrationTarget[]; disabled: boolean;
    remove: () => void; refresh: (gatewayId: string,moduleId: string) => Promise<unknown>;
  } = $props();
  const target = $derived(roleTarget(config,binding.role,targets));
  const controls = $derived(integrationPhysicalControls(binding.kind,target?.state?.moduleEvents?.[target.module.id]));
  const commands = $derived(normalizeIntegrationCatalogue(catalogue).commands);
  const selected = $derived(commands.find(c => c.id === binding.command));
  const interfaces = $derived(selected?.interfaces.filter(i => binding.kind === 'encoder' ? ['fixed_step','variable_step'].includes(i.kind) : i.kind === 'set_state') ?? []);
  function kindChanged(kind: IntegrationInput['kind']) { binding.kind=kind;binding.control=0;binding.command='';binding.interface=kind === 'encoder' ? 'fixed_step' : 'set_state'; }
  function commandChanged(value: string) {
    binding.command=value;
    const command=commands.find(c => c.id === value);
    const supported=command?.interfaces.filter(i => binding.kind === 'encoder' ? ['fixed_step','variable_step'].includes(i.kind) : i.kind === 'set_state');
    binding.interface=(supported?.[0]?.kind ?? (binding.kind === 'encoder' ? 'fixed_step' : 'set_state')) as IntegrationInput['interface'];
  }
</script>

<fieldset {disabled}><legend>Input {binding.id}</legend>
  <div class="fields">
    <label>Binding identifier<input bind:value={binding.id} maxlength="31" spellcheck="false" /></label>
    <label>Device role<select bind:value={binding.role}><option value="">Choose role</option>{#each config.document.roles as role (role.id)}<option value={role.id}>{role.label}</option>{/each}</select></label>
    <label>Input type<select value={binding.kind} onchange={event => kindChanged(event.currentTarget.value as IntegrationInput['kind'])}><option value="button">Momentary button</option><option value="switch">Maintained switch</option><option value="encoder">Encoder</option><option value="axis">Calibrated axis</option></select></label>
    <label>Advertised input<select bind:value={binding.control}><option value={-1}>Choose physical input</option>{#each controls as control (control.value)}<option value={control.value}>{control.label}</option>{/each}</select></label>
    <label>Named provider command<select value={binding.command} onchange={event => commandChanged(event.currentTarget.value)}><option value="">Choose command</option>{#each commands as command (command.id)}<option value={command.id}>{command.label} · {command.id}</option>{/each}</select></label>
    <label>Supported interface<select bind:value={binding.interface}>{#each interfaces as item (item.kind)}<option value={item.kind}>{item.kind}</option>{/each}</select></label>
    {#if binding.kind === 'button' || binding.kind === 'switch'}<label>Pressed state<input type="number" min="0" max="65535" step="1" bind:value={binding.pressedValue} placeholder="1" /></label><label>Released state<input type="number" min="0" max="65535" step="1" bind:value={binding.releasedValue} placeholder="0" /></label>{/if}
    {#if binding.kind === 'encoder' && binding.interface === 'variable_step'}<label>Encoder step<input type="number" min="1" max="65535" step="1" bind:value={binding.step} placeholder="1" /></label>{/if}
    {#if binding.kind === 'axis'}<label>Deadband in provider value units<input type="number" min="0" max="65535" step="1" bind:value={binding.deadband} placeholder="128" /></label>{/if}
  </div>
  <p>{target?.module.directUsb ? 'Disconnect direct USB input before starting.' : target?.module.enabled !== false || target?.module.published !== false ? 'Disable this device’s controller in Devices before starting.' : 'HID disabled · ordered device events'}</p>
  <div class="tools"><button disabled={disabled || !target} onclick={() => target && refresh(target.gatewayId,target.module.id)}>Refresh input capabilities</button><button {disabled} onclick={remove}>Remove binding</button></div>
</fieldset>

<style>
  fieldset { min-width: 0; margin: 12px 0; padding: 14px; border: 1px solid var(--line); border-radius: 6px; } legend { font-size: 12px; font-weight: 600; }
  .fields { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 10px; } label { display: block; margin-top: 8px; color: var(--soft); font-size: 11px; }
  input, select { display: block; width: 100%; min-width: 0; margin-top: 5px; padding: 7px; color: var(--text); background: var(--input-bg); border: 1px solid var(--line); border-radius: 4px; }
  p { color: var(--soft); font-size: 11px; line-height: 1.7; } .tools { display: flex; gap: 10px; margin-top: 12px; }
  button { border: 1px solid var(--line); border-radius: 4px; padding: 7px 10px; background: var(--input-bg); color: var(--text); cursor: pointer; font-size: 11px; } :disabled { opacity: .55; cursor: default; } :focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  @media (max-width: 650px) { .fields { grid-template-columns: 1fr; } }
</style>
