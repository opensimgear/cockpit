import { supportsDeviceContracts } from './gateway-capabilities';
import type { DcsBiosLegacyConfig, DcsBiosStatus } from './dcs-bios';
import type { DeviceAction, DeviceActions, ActionValue } from './device-actions';
import type { GatewayModule, GatewayState } from './gateway';

export type DcsInputKind = 'button' | 'switch' | 'encoder' | 'axis';
export type DcsInputBinding = {
  id: string; gatewayId: string; moduleId: string; kind: DcsInputKind; control: number;
  command: string; interface: 'set_state' | 'fixed_step' | 'variable_step';
  pressedValue?: number; releasedValue?: number; step?: number; deadband?: number;
};
export type DcsOutputFormat = {
  kind: 'boolean' | 'text' | 'integer' | 'fixed'; scale?: number; decimals?: number;
  prefix?: string; suffix?: string; align?: 'left' | 'right'; overflow?: 'truncate' | 'reject';
  unknownText?: string; staleText?: string;
};
export type DcsOutputBinding = {
  id: string; gatewayId: string; moduleId: string; control: string; output?: number;
  actionId: string; format: DcsOutputFormat;
};
export type DcsProfileConfig = {
  schemaVersion: 2; aircraftModule: string; cataloguePath: string;
  commandHost: string; commandPort: number; listenAddress: string; listenPort: number;
  multicastGroup: string; multicastInterface: string; freshnessMs: number;
  inputs: DcsInputBinding[]; outputs: DcsOutputBinding[];
};
export type DcsCatalogueControl = {
  id: string; label: string; category: string;
  inputs: { interface: string; maxValue?: number }[];
  outputs: { index: number; type: 'integer' | 'string'; maxValue?: number; maxLength?: number }[];
};
export type DcsCataloguePage = {
  cataloguePath: string; aircraftModule: string; query: string; kind: 'input' | 'output' | 'all';
  offset: number; total: number; controls: DcsCatalogueControl[];
};
export type PanelInputState = {
  boot: number; stream: number; head: number; buttonCount: number; contactButtonCount?: number;
  held: boolean[]; encoderButtons: number[]; maintainedButtons?: number[]; selectorButtons?: number[][];
  axisCount?: number; axes?: number[]; axisAvailable?: boolean[]; axisNeutral?: number[];
  axisUsage?: number[]; sampledAtMs?: number;
};
export type DcsTarget = { gatewayId: string; module: GatewayModule; state: GatewayState | null; busy?: boolean; label: string };
export type DcsBindingStatus = {
  id: string; gatewayId?: string; moduleId?: string; status?: string; ready?: boolean;
  lastValue?: ActionValue | null; armed?: boolean; needsBaseline?: boolean;
  desired?: ActionValue | ActionValue[] | null;
  applied?: ActionValue | ActionValue[] | null; error?: string | null; commandsSent?: number;
};
export type DcsGatewayMetrics = {
  gatewayId: string; queueDepth?: number; queueHighWater?: number; actionWrites?: number;
  coalesced?: number; latencyMeanMs?: number; latencyMaxMs?: number;
  latencyP95Ms?: number; throughputPerSecond?: number; reconciled?: number;
  inputPollMaxIntervalMs?: number; eventsProcessed?: number;
};

const bytes = (value: string) => new TextEncoder().encode(value).length;
const plainText = (value: string, limit: number) => bytes(value) <= limit && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
const integer = (value: number, minimum: number, maximum: number) => Number.isInteger(value) && value >= minimum && value <= maximum;
export const dcsTargetKey = (target: { gatewayId: string; moduleId: string }) => `${target.gatewayId}/${target.moduleId}`;

/** Upgrade the existing saved master-caution settings without starting or saving. */
export function newDcsProfile(saved?: DcsBiosLegacyConfig | DcsProfileConfig | null): DcsProfileConfig {
  const profile: DcsProfileConfig = {
    schemaVersion: 2, aircraftModule: 'A-10C', cataloguePath: '', commandHost: '127.0.0.1', commandPort: 7778,
    listenAddress: '0.0.0.0', listenPort: 5010, multicastGroup: '239.255.50.10', multicastInterface: '127.0.0.1',
    freshnessMs: 1500, inputs: [], outputs: [],
  };
  if (!saved) return profile;
  for (const key of ['cataloguePath', 'commandHost', 'commandPort', 'listenAddress', 'listenPort', 'multicastGroup', 'multicastInterface', 'freshnessMs'] as const) {
    Object.assign(profile, { [key]: saved[key] });
  }
  if (saved.schemaVersion === 2) {
    profile.aircraftModule = saved.aircraftModule;
    profile.inputs = saved.inputs.map(binding => ({ ...binding }));
    profile.outputs = saved.outputs.map(binding => ({ ...binding, format: { ...binding.format } }));
  } else {
    profile.inputs = [{ id: 'master-caution-button', gatewayId: saved.gatewayId, moduleId: saved.moduleId,
      kind: 'button', control: saved.button, command: 'UFC_MASTER_CAUTION', interface: 'set_state', pressedValue: 1, releasedValue: 0 }];
    profile.outputs = [{ id: 'master-caution-led', gatewayId: saved.gatewayId, moduleId: saved.moduleId,
      control: 'MASTER_CAUTION', output: 0, actionId: saved.ledActionId, format: { kind: 'boolean' } }];
  }
  return profile;
}

function sorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== undefined).sort(([a], [b]) => a.localeCompare(b)).map(([key, item]) => [key, sorted(item)]));
  return value;
}
export function dcsProfileEqual(profile: DcsProfileConfig, saved?: DcsBiosLegacyConfig | DcsProfileConfig | null): boolean {
  return !!saved && JSON.stringify(sorted(profile)) === JSON.stringify(sorted(saved.schemaVersion === 1 ? newDcsProfile(saved) : saved));
}
export function boundDcsTargets(profile: DcsProfileConfig): { gatewayId: string; moduleId: string }[] {
  return [...new Map([...profile.inputs, ...profile.outputs].map(binding => [dcsTargetKey(binding), { gatewayId: binding.gatewayId, moduleId: binding.moduleId }])).values()];
}

/** Form checks improve feedback; native catalogue/route validation remains authoritative. */
export function dcsProfileError(profile: DcsProfileConfig): string {
  if (profile.schemaVersion !== 2 || !/^[A-Za-z0-9_-]{1,63}$/.test(profile.aircraftModule)) return 'Choose an installed aircraft module.';
  if (!profile.cataloguePath.trim() || !plainText(profile.cataloguePath, 4096)) return 'Choose the installed DCS-BIOS doc/json folder.';
  if (profile.inputs.length > 16 || profile.outputs.length > 16 || !profile.inputs.length && !profile.outputs.length) return 'Add up to 16 input and 16 output bindings.';
  for (const port of [profile.commandPort, profile.listenPort]) if (!integer(port, 1, 65535)) return 'UDP ports must be whole numbers from 1 to 65535.';
  if (!integer(profile.freshnessMs, 100, 10000)) return 'Feedback freshness must be from 100 to 10000 ms.';
  for (const address of [profile.commandHost, profile.listenAddress, profile.multicastInterface]) if (!address.trim() || /\s/.test(address)) return 'Enter each UDP address without spaces.';
  if (/\s/.test(profile.multicastGroup)) return 'Enter a multicast group without spaces, or leave it empty for unicast.';
  const ids = new Set<string>();
  for (const binding of [...profile.inputs, ...profile.outputs]) {
    if (!/^[a-zA-Z0-9_.-]{1,31}$/.test(binding.id) || ids.has(binding.id)) return 'Every binding needs a unique identifier of up to 31 letters, digits, dots, underscores or hyphens.';
    ids.add(binding.id);
    if (!/^(?:stm32:[0-9a-f]{24}|pico:OSG-PICO-[0-9a-fA-F]{16})$/.test(binding.gatewayId) || !/^[0-9a-f]{32}$/.test(binding.moduleId)) return 'Choose a supported gateway and module for every binding.';
  }
  const targets = boundDcsTargets(profile);
  if (targets.length > 8 || new Set(targets.map(target => target.gatewayId)).size > 4) return 'One profile supports up to eight modules on four gateways.';
  const physical = new Set<string>(), commands = new Set<string>();
  for (const binding of profile.inputs) {
    if (!['button', 'switch', 'encoder', 'axis'].includes(binding.kind)) return 'Choose a supported physical input type.';
    if (!integer(binding.control, 0, binding.kind === 'axis' || binding.kind === 'encoder' ? 7 : 127)) return 'Choose an advertised physical control for every input.';
    if (!binding.command || !plainText(binding.command, 64)) return 'Choose a named catalogue command for every input.';
    const source = `${dcsTargetKey(binding)}/${binding.kind}/${binding.control}`;
    if (physical.has(source) || commands.has(binding.command)) return 'Each physical input and catalogue command can be bound only once.';
    physical.add(source); commands.add(binding.command);
    if (binding.kind === 'encoder' ? !['fixed_step', 'variable_step'].includes(binding.interface) : binding.interface !== 'set_state') return 'Choose a catalogue interface supported by this input type.';
    for (const value of [binding.pressedValue ?? 1, binding.releasedValue ?? 0, binding.deadband ?? 128]) if (!integer(value, 0, 65535)) return 'Set input values and axis deadband from 0 to 65535.';
    if (!integer(binding.step ?? 1, 1, 65535)) return 'Encoder step must be a whole number from 1 to 65535.';
  }
  const endpoints = new Set<string>();
  for (const binding of profile.outputs) {
    if (!binding.control || !plainText(binding.control, 64) || !binding.actionId) return 'Choose a catalogue output and advertised device action for every output.';
    const endpoint = `${dcsTargetKey(binding)}/${binding.actionId}`;
    if (endpoints.has(endpoint)) return 'Each device output action can be bound only once.';
    endpoints.add(endpoint);
    if (!integer(binding.output ?? 0, 0, 63)) return 'Choose a valid catalogue output index.';
    const format = binding.format;
    if (!['boolean', 'text', 'integer', 'fixed'].includes(format.kind)) return 'Choose a supported output format.';
    if (!Number.isFinite(format.scale ?? 1) || Math.abs(format.scale ?? 1) > 1e6 || !integer(format.decimals ?? 0, 0, 6)) return 'Use an output scale from −1000000 to 1000000 and zero to six decimal places.';
    for (const text of [format.prefix ?? '', format.suffix ?? '']) if (!plainText(text, 16)) return 'Output prefixes and suffixes support up to 16 UTF-8 bytes without control characters.';
    for (const text of [format.unknownText ?? '', format.staleText ?? '']) if (!plainText(text, 63)) return 'Unknown and stale text support up to 63 UTF-8 bytes without control characters.';
    if (format.align && !['left', 'right'].includes(format.align) || format.overflow && !['truncate', 'reject'].includes(format.overflow)) return 'Choose a supported alignment and overflow policy.';
  }
  return '';
}

export function physicalDcsControls(kind: DcsInputKind, events?: PanelInputState): { value: number; label: string }[] {
  if (!events) return [];
  if (kind === 'axis') return Array.from({ length: events.axisCount ?? events.axes?.length ?? 0 }, (_, value) => ({ value, label: `Axis ${value} · calibrated 0–65535` }));
  if (kind === 'encoder') return Array.from({ length: Math.floor(events.encoderButtons.length / 2) }, (_, value) => ({ value, label: `Encoder ${value} · buttons ${events.encoderButtons[value * 2]}/${events.encoderButtons[value * 2 + 1]}` }));
  const count = events.contactButtonCount ?? 0;
  return Array.from({ length: count }, (_, value) => value).filter(value => kind === 'switch' ? events.maintainedButtons?.includes(value) : !events.maintainedButtons?.includes(value)).map(value => ({ value, label: `${kind === 'switch' ? 'Switch' : 'Button'} ${value}` }));
}
export function dcsInputInterfaces(kind: DcsInputKind, control?: DcsCatalogueControl): string[] {
  const allowed = kind === 'encoder' ? ['fixed_step', 'variable_step'] : ['set_state'];
  return [...new Set(control?.inputs.map(input => input.interface).filter(value => allowed.includes(value)) ?? [])];
}
/** Normalization omits unsupported outputs; their original ordinals still identify the wire field. */
export function dcsOutputIndex(kind: DcsOutputFormat['kind'], control?: DcsCatalogueControl): number {
  return control?.outputs.find(output => output.type === (kind === 'text' ? 'string' : 'integer'))?.index ?? 0;
}
export function dcsBindingStatusLabel(status?: DcsBindingStatus): string {
  const labels: Record<string, string> = { ready: 'Ready', unknown: 'Unknown', stale: 'Stale', waiting: 'Waiting', waitingRelease: 'Waiting for release', error: 'Error' };
  return status?.status ? labels[status.status] ?? status.status : status?.ready ? 'Ready' : 'Unknown';
}
/** Keep the unchanged legacy preset's live diagnostics visible without upgrading saved settings. */
export function dcsBindingState(status: DcsBiosStatus | undefined, kind: 'input' | 'output', id: string): DcsBindingStatus | undefined {
  const current = (kind === 'input' ? status?.inputs : status?.outputs)?.find(binding => binding.id === id);
  if (current || status?.config?.schemaVersion !== 1) return current;
  if (kind === 'output' && id === 'master-caution-led') return { id, status: !status.running ? 'unknown' : status.ledValue === null ? 'unknown' : status.feedbackFresh ? 'ready' : 'stale', applied: status.ledValue, error: status.error };
  if (kind === 'input' && id === 'master-caution-button') return { id, ready: !!status.running && !!status.armed,
    status: status.error ? 'error' : !status.running || !status.feedbackFresh || status.needsBaseline ? 'waiting' : status.armed ? 'ready' : 'waitingRelease',
    armed: status.armed, needsBaseline: status.needsBaseline, error: status.error, commandsSent: status.commandsSent };
  return undefined;
}
export function dcsOutputActions(kind: DcsOutputFormat['kind'], snapshot?: DeviceActions): DeviceAction[] {
  if (snapshot?.status !== 'ready') return [];
  return snapshot.actions.filter(action => action.readable && action.semantics === 'setter' && action.parameters.length === 1 &&
    (kind === 'boolean' ? action.parameters[0].type === 'boolean' || action.id === 'led.status.set_mode' && action.parameters[0].type === 'enum' && action.parameters[0].options?.includes('on') && action.parameters[0].options?.includes('off') :
      action.parameters[0].type === 'string'));
}
export function dcsEndpointLabel(action?: DeviceAction): string {
  if (!action) return 'Discover this device’s output actions.';
  return action.display ? `${action.display.columns} × ${action.display.rows} cells · ${action.display.encoding.toUpperCase()} · ${action.parameters[0].maxBytes} bytes` : action.parameters[0].type === 'string' ? `UTF-8 · ${action.parameters[0].maxBytes} bytes` : 'Readable on/off output';
}

export function dcsProfileStartError(profile: DcsProfileConfig, targets: DcsTarget[], saved?: DcsBiosLegacyConfig | DcsProfileConfig | null): string {
  const error = dcsProfileError(profile);
  if (error) return error;
  for (const bound of boundDcsTargets(profile)) {
    const target = targets.find(target => target.gatewayId === bound.gatewayId && target.module.id === bound.moduleId);
    if (!target?.state?.connected || !supportsDeviceContracts(target.state) || !target.module.online || !target.module.boot) return 'Reconnect every bound device before starting.';
    if (target.busy || target.state.reconnecting || target.state.layoutPending) return 'Wait for gateway updates to finish.';
    const inputTarget = profile.inputs.some(binding => dcsTargetKey(binding) === dcsTargetKey(bound));
    if (inputTarget && target.module.directUsb) return `Disconnect ${target.label} from its direct USB input host.`;
    if (inputTarget && (target.module.enabled !== false || target.module.published !== false)) return `Disable ${target.label} and wait for its USB controller to be removed.`;
    if (target.state.claimedModules?.length) return 'End setup sessions on the bound gateways before starting.';
  }
  for (const binding of profile.inputs) {
    const target = targets.find(target => target.gatewayId === binding.gatewayId && target.module.id === binding.moduleId)!;
    const events = target.state?.moduleEvents?.[binding.moduleId];
    if (saved?.schemaVersion === 1 && dcsProfileEqual(profile, saved)) {
      if (saved.button >= target.module.buttonCount || events?.encoderButtons.includes(saved.button)) return `Choose a momentary button within ${target.label} controls.`;
      continue;
    }
    if (events?.boot !== target.module.boot || !physicalDcsControls(binding.kind, events).some(control => control.value === binding.control)) return `Refresh ${target.label} capabilities and choose an available ${binding.kind}.`;
  }
  for (const binding of profile.outputs) {
    const target = targets.find(target => target.gatewayId === binding.gatewayId && target.module.id === binding.moduleId)!;
    const actions = target.state?.moduleActions?.[binding.moduleId];
    if (actions?.boot !== target.module.boot || !dcsOutputActions(binding.format.kind, actions).some(action => action.id === binding.actionId)) return `Refresh ${target.label} capabilities and choose a compatible output.`;
  }
  return '';
}
export function managedDcsActions(status: DcsBiosStatus | undefined, gatewayId: string, moduleId: string): string[] {
  if (!status?.running || !status.config) return [];
  const config = status.config;
  return config.schemaVersion === 1 ? config.gatewayId === gatewayId && config.moduleId === moduleId ? [config.ledActionId] : [] :
    [...new Set(config.outputs.filter(binding => binding.gatewayId === gatewayId && binding.moduleId === moduleId).map(binding => binding.actionId))];
}
export function dcsBindingValue(value: ActionValue | ActionValue[] | null | undefined): string {
  if (value === undefined || value === null) return 'Unknown';
  if (Array.isArray(value)) return value.map(dcsBindingValue).join(' · ');
  return typeof value === 'boolean' ? value ? 'On' : 'Off' : String(value);
}
