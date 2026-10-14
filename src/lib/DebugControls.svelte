<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { DebugInputQueue, debugReady, debugDefinition, rawAxisValue, type DebugOperation } from './debug-inputs';
  import { resetDebugOnExit } from './gateway';
  import type { GatewayRequest, GatewayState, GatewayModule } from './gateway';
  let { state: gateway, module, request, read }: { state: GatewayState; module: GatewayModule; request: (request: GatewayRequest) => Promise<boolean>; read: (request: GatewayRequest) => Promise<boolean> } = $props();
  const device = $derived(gateway.debug?.devices.find(device => device.id === module.id));
  const definition = $derived(device ? debugDefinition(gateway, device) : undefined);
  const ready = $derived(debugReady(gateway, module));
  const inputReady = $derived(ready && !!device?.online);
  const outputs = $derived(gateway.moduleActions?.[module.id]);
  let rawDraft = $state<Record<number, number>>({});
  let failure = $state('');
  let refreshing = false;
  let resetRevision = 0;
  let disposed = false;
  // App keys this component by gateway, device and boot; queued commands keep
  // their original target even if a new inventory snapshot arrives.
  const target = untrack(() => ({ moduleId: module.id,
    boot: gateway.debug?.devices.find(device => device.id === module.id)?.boot ?? module.boot!, gatewayId: gateway.gatewayId }));
  const queue = untrack(() => new DebugInputQueue(target, request));
  async function run(operation: DebugOperation, args: number[] = []) {
    if (!ready) return;
    failure = '';
    const revision = resetRevision;
    if (!await queue.enqueue(operation, args) && revision === resetRevision) failure = 'The input could not be applied. Refresh debug state or reset inputs after reconnecting.';
  }
  function axis(channel: number, value: string) {
    const raw = rawAxisValue(value);
    if (raw === undefined || !inputReady) return;
    rawDraft[channel] = raw;
    void run('axis', [channel, raw]).finally(() => {
      // Keep the newest unsent sample responsive, then return to device
      // readback. An older coalesced request cannot clear a newer slider value.
      if (rawDraft[channel] === raw) delete rawDraft[channel];
    });
  }
  function down(contact: number, event: PointerEvent & { currentTarget: HTMLButtonElement }) {
    if (!inputReady || event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    void queue.press(contact);
  }
  function keyDown(contact: number, event: KeyboardEvent) {
    if (!inputReady || event.repeat || ![' ', 'Enter'].includes(event.key)) return;
    event.preventDefault(); void queue.press(contact);
  }
  function keyUp(contact: number, event: KeyboardEvent) {
    if (![' ', 'Enter'].includes(event.key)) return;
    event.preventDefault(); void queue.release(contact);
  }
  async function reset() { resetRevision++; rawDraft = {}; failure = ''; if (!await queue.reset()) failure = 'Reset did not complete. Reconnect and reset the device before continuing.'; }
  async function refreshInputs(outputsToo = false) {
    if (disposed || !inputReady || refreshing) return;
    refreshing = true;
    const moduleId = module.id;
    try {
      await read({ command: 'events_state', moduleId });
      if (disposed) return;
      if (outputsToo && (!outputs || outputs.boot !== module.boot)) await read({ command: 'action_describe', moduleId });
      else if (outputsToo && outputs?.catalogueRevision) await read({ command: 'action_state', moduleId });
    } finally { refreshing = false; }
  }
  $effect(() => { if (!inputReady && device?.online && device.boot === target.boot && gateway.connected) void queue.releaseHeld(); });
  onMount(() => {
    const release = () => { void queue.releaseHeld(); };
    const exit = () => {
      if (!gateway.connected || device?.boot !== target.boot) return;
      void queue.reset(); if (target.gatewayId) resetDebugOnExit(target.moduleId, target.boot, target.gatewayId);
    };
    const visibility = () => { if (document.hidden) release(); };
    window.addEventListener('blur', release); window.addEventListener('pagehide', exit);
    document.addEventListener('visibilitychange', visibility);
    let tick = 0;
    void refreshInputs(true);
    const timer = setInterval(() => { void refreshInputs(++tick % 2 === 0); }, 500);
    return () => {
      disposed = true;
      window.removeEventListener('blur', release); window.removeEventListener('pagehide', exit);
      document.removeEventListener('visibilitychange', visibility); clearInterval(timer);
      const current = gateway.debug?.devices.find(device => device.id === target.moduleId);
      void queue.close(!!gateway.connected && current?.boot === target.boot && !!current.online);
    };
  });
</script>

<section class="debug-panel" aria-label="Pico debug controls">
  <div class="heading"><div><h2>Pico debug controls</h2><p>Operate raw device inputs through normal calibration and event processing. Inputs remain available with HID disabled and during setup.</p></div><button disabled={!ready} onclick={reset}>Reset inputs</button></div>
  {#if failure}<p class="error" role="alert">{failure}</p>{/if}
  {#if device && definition}
    <div class="lifecycle"><button disabled={!ready} onclick={() => run('online', [device.online ? 0 : 1])}>{device.online ? 'Take device offline' : 'Bring device online'}</button><button disabled={!ready} onclick={() => run('reboot')}>Reboot device</button><label><input type="checkbox" checked={device.directUsb} disabled={!ready || !device.online} onchange={event => run('direct', [event.currentTarget.checked ? 1 : 0])} /> Simulate direct USB input host</label><span>Boot {device.boot} · {device.preset}</span></div>
    {#if !inputReady}<p class="help">{gateway.connected ? 'Bring the device online and wait for the gateway to finish updating controllers.' : 'Reconnect this gateway to operate its virtual devices.'}</p>{/if}
    <fieldset disabled={!inputReady}><legend>Buttons and switches</legend><div class="buttons">
      {#each definition.buttons as mapping, index (index)}
        {#if mapping[1]}<label class="switch"><input type="checkbox" checked={device.contacts.includes(mapping[0])} onchange={event => run('contact', [mapping[0], event.currentTarget.checked ? 1 : 0])} /> Switch {index + 1}<small>Contact {mapping[0]}</small></label>
        {:else}<button class={{ pressed: device.contacts.includes(mapping[0]) }} onpointerdown={event => down(mapping[0], event)} onpointerup={() => queue.release(mapping[0])} onpointercancel={() => queue.release(mapping[0])} onlostpointercapture={() => queue.release(mapping[0])} onkeydown={event => keyDown(mapping[0], event)} onkeyup={event => keyUp(mapping[0], event)} onblur={() => queue.release(mapping[0])} aria-label={`Hold button ${index + 1}, contact ${mapping[0]}`}>Hold button {index + 1}<small>Contact {mapping[0]}</small></button>{/if}
      {:else}<p class="help">No mapped contacts.</p>{/each}
    </div></fieldset>
    {#if definition.axes.length}<fieldset disabled={!inputReady}><legend>Raw axes · 12-bit ADC</legend>{#each definition.axes as mapping, index (index)}<div class="axis"><label for={`raw-axis-${module.id}-${index}`}>Axis {index + 1} · ADC {mapping[0]}</label><input id={`raw-axis-${module.id}-${index}`} aria-label={`Raw axis ${index + 1} slider`} type="range" min="0" max="4095" step="1" value={rawDraft[mapping[0]] ?? device.axes[mapping[0]] ?? 0} oninput={event => axis(mapping[0], event.currentTarget.value)} /><input aria-label={`Raw axis ${index + 1} value`} type="number" min="0" max="4095" step="1" value={rawDraft[mapping[0]] ?? device.axes[mapping[0]] ?? 0} oninput={event => axis(mapping[0], event.currentTarget.value)} /><span>Calibrated: {gateway.moduleEvents?.[module.id]?.axes?.[index] ?? module.axes[index] ?? '—'}</span></div>{/each}<p class="help">Move an axis to its limits while adjusting the raw minimum and maximum in Setup.</p></fieldset>{/if}
    {#if definition.encoders.length}<fieldset disabled={!inputReady}><legend>Encoders</legend>{#each definition.encoders as _, index (index)}<div class="row"><span>Encoder {index + 1}</span><button aria-label={`Encoder ${index + 1} decrement`} onclick={() => run('turn', [index, -1])}>−1 detent</button><button aria-label={`Encoder ${index + 1} increment`} onclick={() => run('turn', [index, 1])}>+1 detent</button></div>{/each}</fieldset>{/if}
    {#if definition.selectors.length}<fieldset disabled={!inputReady}><legend>Three-position selectors</legend>{#each definition.selectors as mapping, index (index)}<label class="row">Selector {index + 1}<select aria-label={`Selector ${index + 1}`} value={String(device.contacts.includes(mapping[0]) ? 0 : device.contacts.includes(mapping[1]) ? 2 : 1)} onchange={event => run('selector', [index, Number(event.currentTarget.value)])}><option value="0">Low</option><option value="1">Center</option><option value="2">High</option></select></label>{/each}</fieldset>{/if}
    {#if definition.hats.length}<fieldset disabled={!inputReady}><legend>Hat directions</legend>{#each definition.hats as _, index (index)}<div class="row"><span>Hat {index + 1}</span>{#each ['Neutral', 'N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'] as label, direction (direction)}<button aria-label={`Hat ${index + 1} ${label}`} onclick={() => run('hat', [index, direction - 1])}>{label}</button>{/each}</div>{/each}</fieldset>{/if}
    <details><summary>Expansion bank availability</summary><p class="help">Simulate an attached or missing expansion bank separately from its configuration mapping.</p><div class="banks">{#each Array.from({ length: 8 }, (_, i) => i) as bank (bank)}<label><input type="checkbox" checked={!!(device.expansionMask & (1 << bank))} disabled={!inputReady} onchange={event => run('expansion', [bank, event.currentTarget.checked ? 1 : 0])} /> Bank {bank + 1}</label>{/each}</div></details>
    {#if outputs?.states.length}<div class="outputs" aria-label="Virtual output readback"><h3>Device output readback</h3>{#each outputs.states as output (output.id)}{@const action = outputs.actions.find(action => action.id === output.id)}<div class="output"><span>{action?.label ?? output.id}</span>{#if action?.display}<output class="display" aria-label={`${action.label} display`}>{outputs.status === 'ready' ? output.values.join(' ') : 'Unknown'}</output>{:else if action?.parameters[0]?.type === 'boolean' || action?.parameters[0]?.type === 'enum'}<span class={['led', { lit: outputs.status === 'ready' && (output.values[0] === true || output.values[0] === 'on') }]} aria-label={`${action?.label ?? output.id} ${outputs.status !== 'ready' ? 'unknown' : output.values[0] === true || output.values[0] === 'on' ? 'on' : 'off'}`}></span><span>{outputs.status === 'ready' ? String(output.values[0]) : 'Unknown'}</span>{:else}<output>{outputs.status === 'ready' ? output.values.join(' · ') : 'Unknown'}</output>{/if}</div>{/each}</div>{/if}
  {:else}<p class="help">Refresh debug state to discover this virtual device’s input mappings.</p>{/if}
  <button disabled={!gateway.connected || gateway.reconnecting} onclick={() => request({ command: 'debug_describe' })}>Refresh debug state</button>
</section>

<style>
  .debug-panel { margin-top: 20px; padding: 22px; border: 1px solid var(--line); border-radius: 8px; background: var(--panel); }
  .heading { display: flex; align-items: start; justify-content: space-between; gap: 12px; } h2 { margin: 0; font-size: 15px; } h3 { font-size: 12px; }
  p, label, span, summary { color: var(--soft); font-size: 11px; line-height: 1.6; } .error { color: var(--amber); }
  button, select, input[type='number'] { padding: 7px 10px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font-size: 11px; }
  button { cursor: pointer; } button small, .switch small { display: block; margin-top: 3px; color: var(--muted); font-size: 10px; } button.pressed { border-color: var(--accent); background: var(--accent-soft); }
  button:focus-visible, input:focus-visible, select:focus-visible, summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; } :disabled { opacity: .55; cursor: default; }
  fieldset { min-width: 0; margin: 14px 0; padding: 12px; border: 1px solid var(--line); border-radius: 5px; } legend { color: var(--soft); font-size: 11px; padding: 0 5px; }
  .buttons, .banks, .lifecycle, .row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; } .lifecycle { margin: 12px 0; } .lifecycle > span { width: 100%; } .switch { padding: 8px; }
  .axis { display: grid; grid-template-columns: 100px minmax(80px, 1fr) 70px; align-items: center; gap: 10px; margin: 12px 0; } .axis > span { grid-column: 2 / 4; } input[type='range'] { width: 100%; } input[type='number'] { width: 100%; }
  .row { margin: 8px 0; } .row > span { min-width: 80px; } details { margin: 14px 0; } summary { cursor: pointer; } .banks { margin-top: 10px; } .banks label { min-width: 90px; }
  .outputs { border-top: 1px solid var(--line); margin-top: 16px; padding-top: 8px; } .output { display: flex; align-items: center; gap: 12px; padding: 8px 0; } .output > span:first-child { flex: 1; }
  .display { min-width: 130px; padding: 8px 12px; background: #131b17; border: 1px solid var(--line); color: #a4e79e; font-family: monospace; white-space: pre-wrap; overflow-wrap: anywhere; } .led { width: 14px; height: 14px; border: 1px solid var(--line); border-radius: 50%; background: var(--input-bg); } .led.lit { background: #edbd42; box-shadow: 0 0 10px #edbd4255; }
  @media (max-width: 700px) { .axis { grid-template-columns: 1fr 70px; } .axis label { grid-column: 1 / 3; } .axis > span { grid-column: 1 / 3; } }
</style>
