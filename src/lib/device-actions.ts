import { supportsDeviceContracts } from './gateway-capabilities';
import type { GatewayModule, GatewayState } from './gateway';

export type ActionValue = boolean | number | string;
export type ActionParameter = {
  id: string; label: string; type: 'boolean' | 'integer' | 'enum' | 'rgb' | 'string';
  minimum?: number; maximum?: number; options?: string[]; maxBytes?: number;
};
export type DeviceAction = {
  id: string; label: string; endpoint: string; semantics: 'setter' | 'oneShot';
  readable: boolean; parameters: ActionParameter[];
  display?: { columns: number; rows: number; encoding: 'ascii' | 'utf8' };
};
export type DeviceActions = {
  boot: number; catalogueRevision?: number; actions: DeviceAction[];
  states: { id: string; values: ActionValue[] }[];
  status: 'ready' | 'unsupported' | 'unknown' | 'error' | 'uncertain'; error?: string | null;
};

/** Actions use the online device route independently of HID input ownership. */
export function actionReady(state: GatewayState | null, module?: GatewayModule): boolean {
  return !!state?.connected && supportsDeviceContracts(state) && !state.reconnecting && !state.layoutPending &&
    !!module?.online && !!module.boot;
}

/** Probe each device boot once. Unsupported is explicit; failures need a retry. */
export function nextActionRead(state: GatewayState | null): string | undefined {
  return state?.modules.find(module => actionReady(state, module) && !state.moduleActions?.[module.id])?.id;
}

export function initialValues(action: DeviceAction, snapshot?: DeviceActions): ActionValue[] {
  const applied = snapshot?.states.find(state => state.id === action.id)?.values;
  return action.parameters.map((parameter, i) => applied?.[i] ?? (parameter.type === 'boolean' ? false :
    parameter.type === 'integer' ? parameter.minimum ?? 0 : parameter.type === 'enum' ? parameter.options?.[0] ?? '' :
    parameter.type === 'rgb' ? '000000' : ''));
}

/** Preserve a missing numeric field as invalid until the user supplies a value. */
export function integerInputValue(value: string): number {
  return value.trim() === '' ? Number.NaN : Number(value);
}

export function actionError(action: DeviceAction, values: ActionValue[]): string {
  if (values.length !== action.parameters.length) return 'This action has incomplete arguments.';
  for (const [index, parameter] of action.parameters.entries()) {
    const value = values[index];
    const valid = parameter.type === 'boolean' ? typeof value === 'boolean' :
      parameter.type === 'integer' ? typeof value === 'number' && Number.isInteger(value) && value >= parameter.minimum! && value <= parameter.maximum! :
      parameter.type === 'enum' ? typeof value === 'string' && !!parameter.options?.includes(value) :
      parameter.type === 'rgb' ? typeof value === 'string' && /^[0-9a-f]{6}$/i.test(value) :
      typeof value === 'string' && new TextEncoder().encode(value).length <= parameter.maxBytes! && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
    if (!valid) return `Check ${parameter.label.toLowerCase()}: its value must match the device’s advertised limits.`;
  }
  return '';
}

export function outputSummary(action: DeviceAction, snapshot?: DeviceActions): string {
  if (!action.readable) return 'This action does not provide state readback.';
  const state = snapshot?.states.find(state => state.id === action.id);
  if (snapshot?.status !== 'ready' || !state) return 'Applied state is unknown.';
  if (!state.values.length) return 'Applied.';
  return action.parameters.map((parameter, index) => `${parameter.label}: ${parameter.type === 'boolean' ? state.values[index] ? 'On' : 'Off' : parameter.type === 'rgb' ? `#${state.values[index]}` : state.values[index]}`).join(' · ');
}
