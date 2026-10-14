<script lang="ts">
  import { onMount } from 'svelte';
  import { cautionLedActions, defaultDcsConfig, dcsConfigEqual, dcsConfigError, dcsStartError, dcsStatusLabel, dcsFeedbackLabel, type DcsBiosConfig } from './dcs-bios';
  import type { GatewayModule, GatewayRequest, GatewayState } from './gateway';
  let { gatewayId, state: gateway, module, busy, request, disableController, canDisable }: {
    gatewayId: string; state: GatewayState | null; module: GatewayModule | undefined; busy: boolean;
    request: (request: GatewayRequest) => Promise<boolean>; disableController: () => void; canDisable: boolean;
  } = $props();
  let draft = $state<DcsBiosConfig>(defaultDcsConfig('', ''));
  let loaded = $state(false);
  let pending = $state(false);
  const status = $derived(gateway?.dcsBios);
  const leds = $derived(cautionLedActions(module ? gateway?.moduleActions?.[module.id] : undefined));
  const configError = $derived(dcsConfigError(draft));
  const startError = $derived(dcsStartError(gateway, module, draft));
  const saved = $derived(!!status?.configured && dcsConfigEqual(draft, status.config));
  const blocked = $derived(!loaded || pending || busy);
  const readOnly = $derived(blocked || !!status?.running);
  const otherDevice = $derived(!!status?.config && (status.config.gatewayId !== gatewayId || status.config.moduleId !== module?.id));
  function restore() { draft = defaultDcsConfig(gatewayId, module?.id ?? '', status?.config, leds); }
  onMount(() => {
    let cancelled = false;
    void request({ command: 'dcs_status' }).finally(() => {
      if (!cancelled) { restore(); loaded = true; }
    });
    return () => { cancelled = true; };
  });
  async function send(command: 'dcs_configure' | 'dcs_start' | 'dcs_stop') {
    if (blocked || (command !== 'dcs_stop' && status?.running)) return;
    if (command === 'dcs_configure' && configError) return;
    if (command === 'dcs_start' && (!saved || startError)) return;
    pending = true;
    try {
      await request(command === 'dcs_configure' ? { command, dcsConfig: { ...draft } } : { command });
    } finally { pending = false; }
  }
</script>

<section aria-label="DCS-BIOS master caution">
  <div class="profile-heading"><div><h3>A-10C master caution</h3><p>Press your panel button to acknowledge master caution. DCS feedback drives its LED.</p></div><span class="status" role="status">{dcsStatusLabel(status)}</span></div>
  <p class="help">Uses the installed DCS-BIOS catalogue’s <code>UFC_MASTER_CAUTION</code> input and <code>MASTER_CAUTION</code> output. Start DCS with DCS-BIOS enabled and load an A-10C mission.</p>
  {#if otherDevice}<p class="notice">The saved profile belongs to {status?.config?.moduleId} on {status?.config?.gatewayId}. Cockpit runs one DCS-BIOS profile. {status?.running ? 'Stop it before saving settings for this device.' : 'Saving here replaces that profile.'}</p>{/if}
  {#if module?.enabled !== false || module?.published !== false}
    <div class="notice"><p>Disable this controller before starting DCS-BIOS to avoid sending both USB HID bindings and DCS-BIOS commands. The device stays online for CAN input events and LED actions.</p><button disabled={blocked || !canDisable || !!status?.running} onclick={disableController}>Disable controller</button></div>
  {:else}<p class="help">USB controller disabled. Check that this device has no existing direct USB binding in DCS.</p>{/if}
  {#if module?.directUsb}<p class="notice">Disconnect the device’s direct USB input connection before starting. Its CAN route must own the inputs.</p>{/if}
  <fieldset disabled={readOnly}>
    <legend>Master-caution binding</legend>
    <label for="dcs-catalogue">Installed DCS-BIOS catalogue folder</label>
    <input id="dcs-catalogue" type="text" bind:value={draft.cataloguePath} placeholder="…/DCS-BIOS/doc/json" spellcheck="false" />
    <p class="help">Enter the local <code>doc/json</code> directory from the DCS-BIOS installation used by DCS.</p>
    <div class="fields">
      <label for="dcs-button">Physical button index<input id="dcs-button" type="number" min="0" max={Math.max(0, (module?.buttonCount ?? 0) - 1)} step="1" bind:value={draft.button} /></label>
      <label for="dcs-led">Feedback LED<select id="dcs-led" bind:value={draft.ledActionId}><option value="">Choose an LED</option>{#each leds as action (action.id)}<option value={action.id}>{action.label}</option>{/each}</select></label>
    </div>
    <p class="help">Button indices start at 0. This device advertises {module?.buttonCount ?? 0} buttons. Boolean LEDs and the board’s on/off status LED are supported.</p>
    {#if !leds.length}<p class="notice">No compatible LED action is available. Use Discover actions in the Device actions panel.</p>{/if}
    <details>
      <summary>UDP connection settings</summary>
      <div class="fields">
        <label for="dcs-command-host">DCS command host<input id="dcs-command-host" type="text" bind:value={draft.commandHost} spellcheck="false" /></label>
        <label for="dcs-command-port">Command port<input id="dcs-command-port" type="number" min="1" max="65535" step="1" bind:value={draft.commandPort} /></label>
        <label for="dcs-listen-address">Feedback listen address<input id="dcs-listen-address" type="text" bind:value={draft.listenAddress} spellcheck="false" /></label>
        <label for="dcs-listen-port">Feedback port<input id="dcs-listen-port" type="number" min="1" max="65535" step="1" bind:value={draft.listenPort} /></label>
        <label for="dcs-multicast-group">Multicast group (optional)<input id="dcs-multicast-group" type="text" bind:value={draft.multicastGroup} spellcheck="false" /></label>
        <label for="dcs-multicast-interface">Multicast interface<input id="dcs-multicast-interface" type="text" bind:value={draft.multicastInterface} spellcheck="false" /></label>
        <label for="dcs-freshness">Feedback freshness (ms)<input id="dcs-freshness" type="number" min="100" max="10000" step="1" bind:value={draft.freshnessMs} /></label>
      </div>
      <p class="help">Defaults connect to DCS on this computer. Use IPv4 addresses and the interface that receives its DCS-BIOS multicast feedback. Leave the multicast group empty to receive unicast feedback.</p>
    </details>
  </fieldset>
  {#if status?.error}<p class="error" role="alert">{status.error}</p>{/if}
  {#if loaded && !status?.running && (configError || startError)}<p class="help">{configError || startError}</p>{/if}
  <div class="toolbar">
    <button disabled={readOnly || !!configError || saved} onclick={() => send('dcs_configure')}>Save settings</button>
    <button class="start" disabled={blocked || !!status?.running || !saved || !!startError} onclick={() => send('dcs_start')}>Start DCS-BIOS</button>
    <button disabled={blocked || !status?.running} onclick={() => send('dcs_stop')}>Stop DCS-BIOS</button>
    <button disabled={readOnly} onclick={restore}>Restore saved</button>
  </div>
  <p class="help">{status?.running ? 'Stop DCS-BIOS before changing this profile.' : saved ? 'Settings saved in Cockpit. Start connects the bridge.' : 'Save these settings before starting. Saving does not start the bridge.'}</p>
  <dl aria-label="DCS-BIOS connection status">
    <div><dt>Aircraft</dt><dd>{status?.aircraft || 'Waiting for aircraft'}</dd></div>
    <div><dt>Mission</dt><dd>{status?.missionActive ? 'Active' : 'Inactive'}</dd></div>
    <div><dt>Feedback</dt><dd>{dcsFeedbackLabel(status)}</dd></div>
    <div><dt>Master-caution LED</dt><dd>{status?.feedbackFresh && status.ledValue !== null ? status.ledValue ? 'On' : 'Off' : 'Unknown'}</dd></div>
    <div><dt>Commands sent</dt><dd>{status?.commandsSent ?? 0}</dd></div>
    <div><dt>Retained event overwrites</dt><dd>{status?.eventOverflow ?? 0}</dd></div>
    <div><dt>Recovered event gaps</dt><dd>{status?.eventGaps ?? 0}</dd></div>
  </dl>
  <p class="help">Old retained events are overwritten as new inputs arrive. Overwrites alone do not mean a DCS command was lost. A missed event gap is counted separately and starts a fresh input baseline.</p>
</section>

<style>
  h3 { margin: 0; font-size: 15px; }
  .profile-heading { display: flex; align-items: start; justify-content: space-between; gap: 10px; }
  .profile-heading p, .help, .notice, .error { color: var(--soft); font-size: 11px; line-height: 1.6; }
  .profile-heading p { margin: 7px 0; }
  .status { padding: 4px 8px; border: 1px solid var(--line); border-radius: 4px; color: var(--soft); font-size: 10px; white-space: nowrap; }
  .notice { margin: 12px 0; padding: 10px; border: 1px solid var(--line); border-radius: 5px; background: var(--input-bg); }
  .notice p { margin: 0 0 8px; }
  .error { color: var(--amber); }
  fieldset { min-width: 0; margin: 16px 0 10px; padding: 12px; border: 1px solid var(--line); border-radius: 5px; }
  legend, label, dt { color: var(--soft); font-size: 11px; }
  label { display: block; margin: 10px 0 5px; }
  input, select { display: block; width: 100%; min-width: 0; margin-top: 5px; padding: 8px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; }
  .fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0 12px; }
  details { margin-top: 15px; padding-top: 12px; border-top: 1px solid var(--line); }
  summary { color: var(--soft); font-size: 11px; font-weight: 600; cursor: pointer; }
  .toolbar { display: flex; flex-wrap: wrap; gap: 8px; }
  button { padding: 7px 10px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; cursor: pointer; }
  button.start { border-color: var(--accent); }
  button:disabled, fieldset:disabled { opacity: .55; }
  button:disabled { cursor: default; }
  button:focus-visible, input:focus-visible, select:focus-visible, summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  dl { margin: 16px 0; border-top: 1px solid var(--line); }
  dl > div { display: flex; justify-content: space-between; gap: 10px; padding: 8px 0; border-bottom: 1px solid var(--line); }
  dd { margin: 0; color: var(--text); font-size: 11px; text-align: right; overflow-wrap: anywhere; }
  code { font-size: 10px; overflow-wrap: anywhere; }
  @media (max-width: 420px) { .fields { grid-template-columns: 1fr; } }
</style>
