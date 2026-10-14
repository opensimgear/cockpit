import type { DeviceAction, DeviceActions } from './device-actions';
import type { GatewayModule, GatewayState } from './gateway';

export type DcsBiosConfig = {
  schemaVersion: 1; cataloguePath: string; gatewayId: string; moduleId: string;
  button: number; ledActionId: string; commandHost: string; commandPort: number;
  listenAddress: string; listenPort: number; multicastGroup: string;
  multicastInterface: string; freshnessMs: number;
};
export type DcsBiosStatus = {
  configured: boolean; running: boolean; config: DcsBiosConfig | null;
  aircraft: string | null; missionActive: boolean; feedbackFresh: boolean;
  feedbackAgeMs: number | null; status: 'stopped' | 'waiting' | 'active' | 'stale' | 'error';
  error: string | null; eventOverflow: number; eventGaps?: number; commandsSent: number; ledValue: boolean | null;
};

/** Only outputs the native master-caution profile can drive are offered. */
export function cautionLedActions(snapshot?: DeviceActions): DeviceAction[] {
  if (snapshot?.status !== 'ready') return [];
  return snapshot.actions.filter(action => action.semantics === 'setter' && action.readable &&
    action.parameters.length === 1 && (action.parameters[0].type === 'boolean' ||
      (action.id === 'led.status.set_mode' && action.parameters[0].type === 'enum' &&
        action.parameters[0].options?.includes('on') && action.parameters[0].options?.includes('off'))));
}

export function defaultDcsConfig(gatewayId: string, moduleId: string, saved?: DcsBiosConfig | null, actions: DeviceAction[] = []): DcsBiosConfig {
  const sameDevice = saved?.gatewayId === gatewayId && saved.moduleId === moduleId;
  return { schemaVersion: 1, cataloguePath: '', button: 0, ledActionId: actions[0]?.id ?? '',
    commandHost: '127.0.0.1', commandPort: 7778, listenAddress: '0.0.0.0', listenPort: 5010,
    multicastGroup: '239.255.50.10', multicastInterface: '127.0.0.1', freshnessMs: 1500,
    ...saved, gatewayId, moduleId, ...(!sameDevice ? { button: 0, ledActionId: actions[0]?.id ?? '' } : {}) };
}

export function dcsConfigEqual(a: DcsBiosConfig, b?: DcsBiosConfig | null): boolean {
  return !!b && (Object.keys(a) as (keyof DcsBiosConfig)[]).every(key => a[key] === b[key]);
}

/** Cheap form checks; native loading and network validation remain authoritative. */
export function dcsConfigError(config: DcsBiosConfig): string {
  if (config.schemaVersion !== 1) return 'These DCS-BIOS settings use an unsupported version.';
  if (!config.cataloguePath.trim() || /[\u0000-\u001f\u007f]/.test(config.cataloguePath)) return 'Choose the installed DCS-BIOS doc/json folder.';
  if (!config.gatewayId || !config.moduleId) return 'Choose a device for this profile.';
  if (!Number.isInteger(config.button) || config.button < 0 || config.button > 127) return 'Choose a physical button index from 0 to 127.';
  if (!config.ledActionId) return 'Choose a compatible LED action.';
  for (const port of [config.commandPort, config.listenPort]) {
    if (!Number.isInteger(port) || port < 1 || port > 65535) return 'UDP ports must be whole numbers from 1 to 65535.';
  }
  if (!Number.isInteger(config.freshnessMs) || config.freshnessMs < 100 || config.freshnessMs > 10000) return 'Feedback freshness must be from 100 to 10000 ms.';
  for (const address of [config.commandHost, config.listenAddress, config.multicastInterface]) {
    if (!address.trim() || /\s/.test(address)) return 'Enter each UDP address without spaces.';
  }
  if (/\s/.test(config.multicastGroup)) return 'Enter the multicast group without spaces, or leave it empty for unicast feedback.';
  return '';
}

/** Recheck current ownership and catalogue immediately before enabling the bridge. */
export function dcsStartError(state: GatewayState | null, module: GatewayModule | undefined, config: DcsBiosConfig): string {
  if (!state?.connected || state.backend !== 'stm32' || !module?.online || !module.boot) return 'Reconnect this CAN device before starting DCS-BIOS.';
  if (state.gatewayId !== config.gatewayId || module.id !== config.moduleId) return 'These settings belong to a different device.';
  if (state.reconnecting || state.layoutPending) return 'Wait for the gateway to finish updating its USB controllers.';
  if (module.directUsb) return 'Disconnect the device’s direct USB input connection before starting DCS-BIOS.';
  if (module.enabled !== false || module.published !== false) return 'Disable this controller and wait until its USB controller is removed to avoid duplicate DCS bindings.';
  if (state.claimedModules?.length) return 'End the gateway’s setup session before starting DCS-BIOS.';
  if (config.button >= module.buttonCount) return 'The selected physical button is outside this device’s current controls.';
  const snapshot = state.moduleActions?.[module.id];
  if (snapshot?.boot !== module.boot || !cautionLedActions(snapshot).some(action => action.id === config.ledActionId)) return 'Discover a compatible LED action for this device before starting DCS-BIOS.';
  return dcsConfigError(config);
}

export function dcsStatusLabel(status?: DcsBiosStatus): string {
  if (!status) return 'Loading status…';
  if (status.error || status.status === 'error') return 'Needs attention';
  if (!status.running) return 'Stopped';
  if (status.status === 'active' && status.feedbackFresh) return 'Active';
  if (status.status === 'stale') return 'Feedback stale';
  return 'Waiting for DCS';
}

export function dcsFeedbackLabel(status?: DcsBiosStatus): string {
  if (!status?.running) return 'Unknown';
  if (status.feedbackAgeMs === null) return 'Waiting for feedback';
  return `${status.feedbackFresh ? 'Fresh' : 'Stale'} · ${status.feedbackAgeMs} ms`;
}
