import type { VisibilityIntent, IsolatedLaunch } from './visibility';
import type { MappingCommand, MappingResult, MappingAssignment, MappingSource, MappingToken, MappingValue, VirtualBinding } from './mappings';
import type { ControllerResult, ControllerCommand } from './controllers';
import type { DeviceKind, Settings } from './devices';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import type { RuntimeSnapshot, RuntimeConfig } from './runtime-config';
import type { DeviceActions, ActionValue } from './device-actions';
import type { DcsBiosConfig, DcsBiosStatus } from './dcs-bios';
import type { DebugGateway, DebugOperation } from './debug-inputs';
import type { PanelInputState } from './dcs-profile';
import type { IntegrationConfig, IntegrationState, IntegrationCommandName, TelemetryFrame } from './integration-profile';
import type { GamesState, GameProfile, GameId, GameCommand } from './games';

export type GatewayBackend = 'pico' | 'stm32';
export type GatewayPort = { gatewayId?: string; path: string; manufacturer?: string; friendlyName?: string; serialNumber?: string; vendorId?: string; productId?: string; backend?: GatewayBackend };
export type GatewayModule = {
  id: string;
  name: string;
  kind: DeviceKind;
  online: boolean;
  registered: boolean;
  active: boolean;
  enabled?: boolean;
  outputOnly?: boolean;
  published?: boolean;
  descriptorIndex?: number;
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
  running: boolean; needsReconnect: boolean; claimedModules?: string[]; modules: GatewayModule[];
  backend?: GatewayBackend; connected?: boolean; port?: string | null; ports?: GatewayPort[];
  gatewayId?: string; gatewaySerial?: string; protocolVersion?: number; firmwareVersion?: string; reconnecting?: boolean;
  epoch?: number; canProtocol?: number; maxModules?: number; needsApply?: boolean;
  maxRemembered?: number; maxControllers?: number; activeGeneration?: number; pendingGeneration?: number;
  lifecycle?: 'automatic'; layoutPending?: boolean; layoutError?: string | null;
  admissionFailure?: { identity: string; error: string } | null; rejectedAnnouncements?: number;
  hidNaming?: { supported: boolean; applied: boolean; error?: string };
  moduleConfigs?: Record<string, RuntimeSnapshot>;
  moduleActions?: Record<string, DeviceActions>;
  moduleEvents?: Record<string, PanelInputState>;
  dcsBios?: DcsBiosStatus;
  debug?: DebugGateway;
  integration?: IntegrationState;
  games?: GamesState;
  controllers?: ControllerResult;
  mappings?: MappingResult;
};
export type GatewayCommand = MappingCommand | ControllerCommand | GameCommand | IntegrationCommandName | 'action_claim' | 'action_renew' | 'action_release' | 'action_ownership' | 'action_invoke_owned' | 'status' | 'start' | 'stop' | 'register' | 'reconnect' | 'apply' | 'save' | 'unregister' | 'enable' | 'disable' | 'forget' | 'update-profile' | 'configure' | 'claim' | 'release' | 'ports' | 'connect' | 'disconnect' | 'remove' | 'config_read' | 'config_apply' | 'config_apply_save' | 'config_save' | 'config_discard' | 'config_reset' | 'action_describe' | 'action_state' | 'action_invoke' | 'events_state' | 'events_read' | 'dcs_status' | 'dcs_catalogue' | 'dcs_configure' | 'dcs_start' | 'dcs_stop' | 'debug_describe' | 'debug_invoke';
export type GatewayRequest = { launch?: IsolatedLaunch; visibility?: {mode: string; sourceIds: string[]} | null; sourceCode?: string; intent?: VisibilityIntent; sourceIds?: string[]; sessionId?: string; jobId?: string; enabled?: boolean; bindings?: VirtualBinding[]; uploadId?: string; size?: number; bytes?: number[]; assignments?: MappingAssignment[]; source?: MappingSource; token?: MappingToken; changes?: { control: string; value: MappingValue }[]; sourceId?: string; observedId?: string; name?: string; role?: string; gameProfile?: GameProfile; gameId?: GameId; profileId?: string; executable?: string | null; acceptBindingChanges?: boolean; preserveCurrentSetup?: boolean; userFolder?: string; connection?: Record<string, unknown>; integrationConfig?: IntegrationConfig; bindingId?: string; durationMs?: number; resume?: boolean; revision?: number; telemetryFrame?: TelemetryFrame; ownershipToken?: string; leaseId?: number; ttlMs?: number; command: GatewayCommand; gatewayId?: string; backend?: GatewayBackend; moduleId?: string; settings?: Settings; port?: string; config?: RuntimeConfig; boot?: number; catalogueRevision?: number; actionId?: string; values?: ActionValue[]; dcsConfig?: DcsBiosConfig; stream?: number; after?: number | string; cataloguePath?: string; aircraftModule?: string; kind?: 'input' | 'output' | 'all'; query?: string; offset?: number; limit?: number; debugOperation?: DebugOperation; debugArgs?: number[]; preset?: string };

const clientId = crypto.randomUUID();
const browserEndpoint = '/api/gateway/';

export async function pickGameExecutable(gameId: GameId, currentPath?: string | null): Promise<string | null> {
  if (!isTauri()) throw new Error('The native file picker is available in the Cockpit desktop app.');
  return invoke<string | null>('pick_game_executable', { gameId, currentPath: currentPath ?? null });
}
type GatewayResult = { ok: boolean; state?: GatewayState; error?: string };
// Each gateway owns its command queue. A blocked serial connection must not
// hold up discovery or requests for another gateway, including the same backend.
const requestQueues = new Map<string, Promise<unknown>>();
function queuedRequest(request: GatewayRequest, operation: () => Promise<GatewayState>): Promise<GatewayState> {
  const key = request.command === 'controller_visibility_restore' ? 'visibility-stop' : request.command === 'mapping_stop' ? 'mapping-stop' : request.command === 'mapping_virtual_inspect' ? 'mapping-inspect' : request.command.startsWith('mapping_') ? 'mapping' : request.command.startsWith('controller_') ? 'controllers' : request.command.startsWith('game_') ? 'games' : request.command.startsWith('dcs_') || request.command.startsWith('integration_') ? 'integration' : request.gatewayId ?? 'inventory';
  const previous = requestQueues.get(key) ?? Promise.resolve();
  const pending = previous.catch(() => {}).then(operation);
  const settled = pending.then(() => {}, () => {});
  requestQueues.set(key, settled);
  void settled.then(() => { if (requestQueues.get(key) === settled) requestQueues.delete(key); });
  return pending;
}
export class GatewayError extends Error {
  constructor(message: string, public state?: GatewayState) { super(message); }
}
function resultState(result: GatewayResult): GatewayState {
  if (!result.ok || !result.state) throw new GatewayError(result.error ?? 'The gateway request failed.', result.state);
  return result.state;
}

export function gatewayRequest(request: GatewayRequest): Promise<GatewayState> {
  return queuedRequest(request, async () => {
    if (isTauri()) {
      let result: GatewayResult;
      try {
        result = await invoke<GatewayResult>('gateway_request', { request: { ...request, clientId } });
      } catch (error) {
        throw new GatewayError(error instanceof Error ? error.message : String(error));
      }
      return resultState(result);
    }
    const response = await fetch(browserEndpoint, {
      method: 'POST', headers: { 'Content-Type': 'application/json', 'X-OSG-Client': '1' },
      body: JSON.stringify({ ...request, clientId }), signal: AbortSignal.timeout(135_000),
      keepalive: request.command === 'debug_invoke',
    });
    if (!response.headers.get('content-type')?.includes('application/json')) throw new GatewayError('Start the browser development app with pnpm web:dev to enable its gateway bridge.');
    const result: GatewayResult = await response.json();
    if (!response.ok) throw new GatewayError(result.error ?? 'The gateway request failed.', result.state);
    return resultState(result);
  });
}

function requestOnExit(request: GatewayRequest) {
  if (isTauri()) {
    void gatewayRequest(request).catch(() => {});
    return;
  }
  void fetch(browserEndpoint, {
    method: 'POST', headers: { 'Content-Type': 'application/json', 'X-OSG-Client': '1' },
    body: JSON.stringify({ ...request, clientId }), keepalive: true,
  }).catch(() => {});
}

export function releaseOnExit(moduleId: string, gatewayId: string) {
  requestOnExit({ command: 'release', moduleId, gatewayId });
}

/** Neutralize raw debug controls even when the browser unloads before its
 * normal command queue can finish. The live boot guards replacement devices. */
export function resetDebugOnExit(moduleId: string, boot: number, gatewayId: string) {
  requestOnExit({ command: 'debug_invoke', moduleId, boot, gatewayId, debugOperation: 'neutral', debugArgs: [] });
}

export function openCockpitLink(event: MouseEvent): Promise<void> | undefined {
  if (!isTauri()) return;
  event.preventDefault();
  const href = (event.currentTarget as HTMLAnchorElement).getAttribute('href');
  if (href?.startsWith('https://')) return openUrl(href);
  return Promise.reject(new Error('This link could not be opened.'));
}
