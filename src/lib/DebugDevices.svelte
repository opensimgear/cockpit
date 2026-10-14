<script lang="ts">
  import { debugPresetLabels } from './debug-inputs';
  import type { GatewayRequest, GatewayState } from './gateway';
  let { state: gateway, busy, request }: { state: GatewayState; busy: boolean; request: (request: GatewayRequest) => Promise<boolean> } = $props();
  let preset = $state('master-caution');
  const available = $derived(gateway.debug?.presets ?? []);
  const full = $derived((gateway.debug?.devices.length ?? 0) >= (gateway.debug?.maxDevices ?? 0));
  async function add() {
    if (busy || full || !gateway.connected || gateway.layoutPending || gateway.reconnecting) return;
    await request({ command: 'debug_invoke', debugOperation: 'add', preset: available.includes(preset) ? preset : available[0], debugArgs: [] });
  }
</script>

<div class="debug-devices" aria-label="Pico virtual devices">
  <div><strong>Pico debug devices</strong><p>Add virtual inputs and DCS-BIOS panels to test configuration, USB controllers and simulator feedback.</p></div>
  <label>Virtual device preset<select bind:value={preset} disabled={busy || !gateway.connected || full}>{#each available as value (value)}<option {value}>{debugPresetLabels[value] ?? value}</option>{/each}</select></label>
  <button disabled={busy || !gateway.connected || gateway.reconnecting || gateway.layoutPending || full || !available.length} onclick={add}>Add virtual device</button>
  <span>{gateway.debug?.devices.length ?? 0} / {gateway.debug?.maxDevices ?? 0} virtual devices{full ? ' · capacity reached' : ''}</span>
  <div class="roster" aria-label="Virtual device lifecycle">{#each gateway.debug?.devices ?? [] as device (device.id)}<div><strong>{device.definition.name}</strong><span>{device.online ? 'Online' : 'Offline'} · boot {device.boot}</span><button disabled={busy || !gateway.connected || gateway.reconnecting || gateway.layoutPending} onclick={() => request({ command: 'debug_invoke', moduleId: device.id, boot: device.boot, debugOperation: 'online', debugArgs: [device.online ? 0 : 1] })}>{device.online ? 'Take offline' : 'Bring online'}</button><button disabled={busy || !gateway.connected || gateway.reconnecting || gateway.layoutPending} onclick={() => request({ command: 'debug_invoke', moduleId: device.id, boot: device.boot, debugOperation: 'reboot', debugArgs: [] })}>Reboot</button></div>{/each}</div>
</div>

<style>
  .debug-devices { display: flex; flex-wrap: wrap; align-items: end; gap: 12px; padding: 16px; margin-top: 16px; border: 1px solid var(--line); border-radius: 6px; background: var(--input-bg); }
  .debug-devices > div { flex: 1; min-width: 220px; } strong { font-size: 12px; } p, label, span { color: var(--soft); font-size: 11px; line-height: 1.6; } p { margin-bottom: 0; }
  label { display: block; } select { display: block; margin-top: 5px; width: 100%; }
  button, select { padding: 8px 12px; border: 1px solid var(--line); border-radius: 4px; background: var(--panel); color: var(--text); font-size: 11px; } button { cursor: pointer; }
  span { width: 100%; } :disabled { opacity: .55; cursor: default; } button:focus-visible, select:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .roster { flex: 1 1 100%; } .roster > div { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; border-top: 1px solid var(--line); padding-top: 10px; margin-top: 10px; } .roster strong { flex: 1; } .roster span { width: auto; }
</style>
