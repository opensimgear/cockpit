import type { DeviceKind, Settings } from './devices';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';

export type GatewayPort = { path: string; manufacturer?: string; serialNumber?: string; vendorId?: string; productId?: string };
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
  buttons: number[];
  axes: number[];
  serialNumber: string;
  hidInterface?: number | null;
  usbSerialNumber?: string;
  settings: Settings;
};
export type GatewayState = {
  running: boolean; needsReconnect: boolean; claimedModules?: string[]; modules: SimulatedModule[];
  backend?: 'pico'; connected?: boolean; port?: string | null; ports?: GatewayPort[];
  gatewaySerial?: string; protocolVersion?: number; firmwareVersion?: string; reconnecting?: boolean;
  hidNaming?: { supported: boolean; applied: boolean; error?: string };
};
export type GatewayCommand = 'status' | 'start' | 'stop' | 'input' | 'register' | 'reconnect' | 'presence' | 'configure' | 'claim' | 'release' | 'ports' | 'connect' | 'disconnect' | 'add' | 'remove';
export type GatewayRequest = { command: GatewayCommand; moduleId?: string; buttons?: number[]; axes?: number[]; online?: boolean; settings?: Settings; port?: string; module?: NewModule };

const clientId = crypto.randomUUID();
type GatewayResult = { ok: boolean; state?: GatewayState; error?: string };
let nativeQueue: Promise<unknown> = Promise.resolve();
function nativeRequest(request: GatewayRequest): Promise<GatewayResult> {
  const operation = nativeQueue.then(() => invoke<GatewayResult>('pico_gateway', { request: { ...request, clientId } }));
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
  const response = await fetch('/api/pico-gateway/', {
    method: 'POST', headers: { 'Content-Type': 'application/json', 'X-OSG-Simulator': '1' },
    body: JSON.stringify({ ...request, clientId }), signal: AbortSignal.timeout(135_000),
  });
  if (!response.headers.get('content-type')?.includes('application/json')) throw new Error('Start the browser prototype with pnpm web:dev to enable its gateway bridge.');
  const result: GatewayResult = await response.json();
  if (!response.ok) throw new GatewayError(result.error ?? 'The gateway request failed.', result.state);
  return resultState(result);
}

export function releaseOnExit(moduleId: string) {
  if (isTauri()) {
    void nativeRequest({ command: 'release', moduleId }).catch(() => {});
    return;
  }
  void fetch('/api/pico-gateway/', {
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
