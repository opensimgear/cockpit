import type { DeviceKind, Settings } from './devices';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import type { RuntimeSnapshot, RuntimeConfig } from './runtime-config';

export type GatewayBackend = 'pico' | 'stm32';
export type GatewayPort = { path: string; manufacturer?: string; friendlyName?: string; serialNumber?: string; vendorId?: string; productId?: string; backend?: GatewayBackend };
export type NewModule = { id: string; name: string; kind: Exclude<DeviceKind, 'gateway'>; buttonCount: number; axisCount: number };
export type SimulatedModule = {
  id: string;
  name: string;
  kind: DeviceKind;
  online: boolean;
  registered: boolean;
  active: boolean;
  buttonCount: number;
  axisCount: number;
  hatCount?: number;
  buttons: number[];
  axes: number[];
  hats?: number[];
  directUsb?: boolean;
  profileMatch?: boolean;
  pendingProfileMatch?: boolean;
  pendingSlot?: number;
  activeSlot?: number;
  reportId?: number | null;
  canConfigure?: boolean;
  boot?: number;
  configurationGeneration?: number;
  serialNumber: string;
  hidInterface?: number | null;
  usbSerialNumber?: string;
  settings: Settings;
};
export type GatewayState = {
  running: boolean; needsReconnect: boolean; claimedModules?: string[]; modules: SimulatedModule[];
  backend?: GatewayBackend; connected?: boolean; port?: string | null; ports?: GatewayPort[];
  gatewaySerial?: string; protocolVersion?: number; firmwareVersion?: string; reconnecting?: boolean;
  epoch?: number; canProtocol?: number; maxModules?: number; needsApply?: boolean;
  hidNaming?: { supported: boolean; applied: boolean; error?: string };
  moduleConfigs?: Record<string, RuntimeSnapshot>;
};
export type GatewayCommand = 'status' | 'start' | 'stop' | 'input' | 'register' | 'reconnect' | 'apply' | 'save' | 'unregister' | 'presence' | 'configure' | 'claim' | 'release' | 'ports' | 'connect' | 'disconnect' | 'add' | 'remove' | 'config_read' | 'config_apply' | 'config_apply_save' | 'config_save' | 'config_discard' | 'config_reset';
export type GatewayRequest = { command: GatewayCommand; backend?: GatewayBackend; moduleId?: string; buttons?: number[]; axes?: number[]; online?: boolean; settings?: Settings; port?: string; module?: NewModule; config?: RuntimeConfig };

const clientId = crypto.randomUUID();
const emulator = () => '__OSG_NATIVE_EMULATOR__' in globalThis && !!(globalThis as { __OSG_NATIVE_EMULATOR__?: boolean }).__OSG_NATIVE_EMULATOR__;
const browserEndpoint = () => emulator() ? '/api/native-gateway/' : '/api/pico-gateway/';
type GatewayResult = { ok: boolean; state?: GatewayState; error?: string };
let nativeQueue: Promise<unknown> = Promise.resolve();
let nativeBackend: GatewayBackend = 'pico';
let nativeConnected = false;
function nativeRequest(request: GatewayRequest): Promise<GatewayResult> {
  const operation = nativeQueue.then(async () => {
    const { backend: requestedBackend, ...payload } = request;
    const call = (backend: GatewayBackend, value: GatewayRequest) => invoke<GatewayResult>(`${backend === 'stm32' ? 'stm32' : 'pico'}_gateway`, { request: { ...value, clientId } });
    if (!requestedBackend && !nativeConnected && (request.command === 'status' || request.command === 'ports')) {
      const results = await Promise.allSettled([call('pico', payload), call('stm32', payload)]);
      const available = results.flatMap(result => result.status === 'fulfilled' ? [result.value] : []);
      const result = available.find(result => result.state?.connected) ?? available.find(result => result.ok) ?? available[0];
      if (!result) throw (results[0] as PromiseRejectedResult).reason;
      const ports = new Map(available.flatMap(result => result.state?.ports ?? []).map(port => [port.path, port]));
      if (result.state) result.state = { ...result.state, ports: [...ports.values()] };
      nativeBackend = result.state?.backend ?? 'pico';
      nativeConnected = !!result.state?.connected;
      return result;
    }
    const backend = requestedBackend ?? nativeBackend;
    if (request.command === 'connect' && backend !== nativeBackend && nativeConnected) {
      const closed = await call(nativeBackend, { command: 'disconnect' });
      if (!closed.ok) return closed;
      nativeConnected = false;
    }
    const result = await call(backend, payload);
    nativeBackend = result.state?.backend ?? backend;
    nativeConnected = !!result.state?.connected;
    return result;
  });
  nativeQueue = operation.catch(() => {});
  return operation;
}
export class GatewayError extends Error {
  constructor(message: string, public state?: GatewayState) { super(message); }
}
function resultState(result: GatewayResult): GatewayState {
  if (!result.ok || !result.state) throw new GatewayError(result.error ?? 'The gateway request failed.', result.state);
  return result.state;
}

export async function gatewayRequest(request: GatewayRequest): Promise<GatewayState> {
  if (isTauri()) {
    let result: GatewayResult;
    try {
      result = await nativeRequest(request);
    } catch (error) {
      throw new GatewayError(error instanceof Error ? error.message : String(error));
    }
    return resultState(result);
  }
  if (request.backend === 'stm32' && !emulator()) throw new GatewayError('Use the Cockpit desktop app to connect a CAN gateway.');
  const payload = { ...request };
  delete payload.backend;
  const response = await fetch(browserEndpoint(), {
    method: 'POST', headers: { 'Content-Type': 'application/json', 'X-OSG-Simulator': '1' },
    body: JSON.stringify({ ...payload, clientId }), signal: AbortSignal.timeout(135_000),
  });
  if (!response.headers.get('content-type')?.includes('application/json')) throw new Error('Start the browser development app with pnpm web:dev to enable its gateway bridge.');
  const result: GatewayResult = await response.json();
  if (!response.ok) throw new GatewayError(result.error ?? 'The gateway request failed.', result.state);
  return resultState(result);
}

export function releaseOnExit(moduleId: string) {
  if (isTauri()) {
    void nativeRequest({ command: 'release', moduleId }).catch(() => {});
    return;
  }
  void fetch(browserEndpoint(), {
    method: 'POST', headers: { 'Content-Type': 'application/json', 'X-OSG-Simulator': '1' },
    body: JSON.stringify({ command: 'release', moduleId, clientId }), keepalive: true,
  }).catch(() => {});
}

export function openCockpitLink(event: MouseEvent): Promise<void> | undefined {
  if (!isTauri()) return;
  event.preventDefault();
  const href = (event.currentTarget as HTMLAnchorElement).getAttribute('href');
  if (href === '/pico-flashing.html') return invoke<void>('open_pico_flashing_guide');
  if (href?.startsWith('https://')) return openUrl(href);
  return Promise.reject(new Error('This link could not be opened.'));
}
