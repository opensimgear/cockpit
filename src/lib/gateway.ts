import type {RetiredRecoveryCommand,RetiredRecoveryStatus,RetiredJournalRef,RetiredProvisionRef} from './profile-retired-recovery';
import type { CompanionCommand, CompanionHostState } from './companions';
import type { VisibilityIntent } from './visibility';
import type { ControllerResult, ControllerCommand } from './controllers';
import type { DeviceKind, Settings } from './devices';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import type { RuntimeSnapshot, RuntimeConfig } from './runtime-config';
import type { DeviceActions, ActionValue } from './device-actions';
import type { DebugGateway, DebugOperation } from './debug-inputs';
import type { PanelInputState } from './panel-input-metadata';
import { updateGameCatalogue, type GamesState, type GameId, type GameCommand } from './games';
import type { ProfileCommand, ProfileResult, ProfileDocument, ProfileLocalAssignment } from './profiles';
import type { ProfileCompilationCommand, ProfileCompilationResult } from './profile-compilation';
import type { ProfileInspectionPage, ProfileInspectionRequest } from './profile-inspection';
import type { ProfileTopologyPage, ProfileTopologyRequest } from './profile-activity';
import type { ProfileLearningCommand, ProfileLearningStatus } from './profile-learning';
import type { ProfileAutomaticCommand } from './profile-automatic';
import type {GameProfileContextRequest} from './game-profile-context';
import type { ProfileCompanionAuthoringCommand, ProfileCompanionAuthoringResult } from './profile-companion-authoring';
import type { ProfileInventoryCommand, ProfileResourcesResult, ResourceToken, ResourceAccess } from './profile-inventory';
import type { ModuleUsbCommand, ModuleUsbResult, UsbRouteToken } from './module-usb';
import type { NativeSystemInventory } from './system-inventory-providers';
import type {DesktopCommand,DesktopStatus,DesktopScope}from'./profile-desktop-runtime';
import type{DesktopPreviewResult,DesktopPreviewFrame}from'./profile-desktop-preview';
import type {GameTelemetryReport} from './game-telemetry';

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
  activity?: import('./activity/model').ActivityPage;
  activitySettings?: import('./activity/model').ActivitySettings;
  activityDeleted?: number;
  uiPreferences?: import('./persistence/ui-preferences').UiSnapshot | { revision: number; value: unknown };
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
  debug?: DebugGateway;
  games?: GamesState;
  gameMetadata?: import('./game-metadata').GameMetadataResponse;
  gameArtwork?: {gameId:string;items?:import('./games').GameArtworkOption[];image?:string;total?:number;nextOffset?:number|null};
  gameTelemetry?: GameTelemetryReport;
  controllers?: ControllerResult;
  widgetLibrary?:import('./compiled-companion-library').WidgetSourceBundle;
  widgetSaved?:{id:string;version:number;title:string;saved:boolean};
  companions?: CompanionHostState;
  profiles?: ProfileResult;
  profileResources?: ProfileResourcesResult;
  profileCompilation?: ProfileCompilationResult;
  profileInspection?: ProfileInspectionPage;
  profileTopology?: ProfileTopologyPage;
  profileLearning?:ProfileLearningStatus;
  profileCompanionAuthoring?:ProfileCompanionAuthoringResult;
  moduleUsb?: ModuleUsbResult;
  retiredRecovery?: RetiredRecoveryStatus;
  systemInventory?: NativeSystemInventory;
  desktop?:DesktopStatus;
  localSettings?:import('./ui-settings-guard').LocalSettingsSnapshot;
  profileDesktopPreview?:DesktopPreviewResult;
  profileActivation?:import('./profile-runtime').NativeProfileActivation;
};
export type GatewayCommand = import('./activity/model').ActivityRequest['command'] | 'ui_preferences_get' | 'ui_preferences_save' | 'widget_library_get' | 'widget_library_merge' | 'widget_save' | 'profile_local_settings_get' | 'profile_local_settings_save' | 'game_telemetry_inspect' | GameProfileContextRequest['command'] | 'profile_activation_current' | 'profile_activation_status' | 'profile_activation_cancel' | 'profile_desktop_preview' | DesktopCommand | 'system_inventory' | RetiredRecoveryCommand | 'profile_game_launch' | ProfileTopologyRequest['command'] | ProfileCompanionAuthoringCommand | ProfileAutomaticCommand | ProfileLearningCommand | ProfileInspectionRequest['command'] | ProfileCompilationCommand | ModuleUsbCommand | ProfileInventoryCommand | ProfileCommand | CompanionCommand | ControllerCommand | GameCommand | 'action_claim' | 'action_renew' | 'action_release' | 'action_ownership' | 'action_invoke_owned' | 'status' | 'start' | 'stop' | 'register' | 'reconnect' | 'apply' | 'save' | 'unregister' | 'enable' | 'disable' | 'forget' | 'update-profile' | 'configure' | 'claim' | 'release' | 'ports' | 'connect' | 'disconnect' | 'remove' | 'config_read' | 'config_apply' | 'config_apply_save' | 'config_save' | 'config_discard' | 'config_reset' | 'action_describe' | 'action_state' | 'action_invoke' | 'events_state' | 'events_read' | 'debug_describe' | 'debug_invoke';
type ProfileRequestFields = { vehicleId?:string; activationId?:string; expectedActiveOwner?:import('./profile-runtime').NativeProfileOwner|null; frames?:DesktopPreviewFrame[];advances?:string[]; scope?:DesktopScope|import('./ui-settings-guard').LocalSettingsScope;expectedSettingsRevision?:string; publicationHash?:string;priority?:number;learnId?:string;generation?:string;capabilityRevision?:string; owner?: ProfileInspectionRequest['owner']; nodeId?: string; id?: string; document?: ProfileDocument; expectedRevision?: number | null; expectedSetupRevision?: string; mode?: 'manual' | 'automatic'; selectionSource?: 'manual' | 'automatic'; confirm?: boolean; resourceId?: string; portId?: string; resourceToken?: ResourceToken; access?: ResourceAccess; compilationId?: string; section?: string };
export type GatewayRequest = ProfileRequestFields & { activityFilter?: import('./activity/model').ActivityQuery; retentionDays?: number; artworkId?:string; value?: unknown; bundle?:import('./compiled-companion-library').WidgetSourceBundle; url?:string; imageData?:string; crop?:import('./games').GameCrop; tags?: Record<string,string>; expectedTags?: Record<string,string>; tagIds?: string[]; expectedTagIds?: string[]; search?: string; group?: string; background?: boolean; observedAt?: number; key?: string; route?: UsbRouteToken; save?: boolean; companion?: Record<string, unknown>; sourceCode?: string; intent?: VisibilityIntent; sessionId?: string; jobId?: string; enabled?: boolean; uploadId?: string; size?: number; bytes?: number[]; assignments?: ProfileLocalAssignment[]; reference?: RetiredJournalRef | RetiredProvisionRef; sourceId?: string; observedId?: string; name?: string; role?: string; gameId?: GameId; path?: string | null; positionX?: number; positionY?: number; candidateId?: string; profileId?: string; rememberChoice?:boolean; executable?: string | null; preserveCurrentSetup?: boolean; userFolder?: string; connection?: Record<string, unknown>; bindingId?: string; durationMs?: number; resume?: boolean; revision?: number; ownershipToken?: string; leaseId?: number; ttlMs?: number; command: GatewayCommand; gatewayId?: string; backend?: GatewayBackend; moduleId?: string; settings?: Settings | Record<string,unknown>; port?: string; config?: RuntimeConfig; boot?: number; catalogueRevision?: number; actionId?: string; values?: ActionValue[]; stream?: number; after?: number | string; cataloguePath?: string; aircraftModule?: string; kind?: 'input' | 'output' | 'all'; query?: string; offset?: number; limit?: number; debugOperation?: DebugOperation; debugArgs?: number[]; preset?: string };


const clientId = crypto.randomUUID();
const browserEndpoint = '/api/gateway/';

export async function pickGameExecutable(gameId: GameId | '', currentPath?: string | null): Promise<string | null> {
  if (!isTauri()) throw new Error('The native file picker is available in the Cockpit desktop app.');
  return invoke<string | null>('pick_game_executable', { gameId, currentPath: currentPath ?? null });
}

export async function pickGameImage(currentPath?: string | null): Promise<string | null> {
  if (!isTauri()) throw new Error('Enter an image path, or choose an image in the Cockpit desktop app.');
  return invoke<string | null>('pick_game_image', { currentPath: currentPath ?? null });
}

export async function pickGameIntegrationFolder(kind: 'saved-games' | 'catalogue', currentPath?: string | null): Promise<string | null> {
  if (!isTauri()) throw new Error('The native folder picker is available in the Cockpit desktop app.');
  return invoke<string | null>('pick_game_integration_folder', { kind, currentPath: currentPath ?? null });
}
type GatewayResult = { ok: boolean; state?: GatewayState; error?: string };
// Each gateway owns its command queue. A blocked serial connection must not
// hold up discovery or requests for another gateway, including the same backend.
const requestQueues = new Map<string, Promise<unknown>>();
const usbRoutes = new Map<string, { epoch: number; pending: number }>();
// One native ProfileSession owns every profile. An opaque identity cancels
// unsent Apply requests without accumulating per-profile cancellation state.
let profileApplyEpoch = {};
let profileControlBarrier: Promise<void> = Promise.resolve();
let learningEpoch={};
let retiredRecoveryEpoch={};
function learningInterrupt(request:GatewayRequest):boolean{
  const command=request.command;
  if(command.startsWith('profile_'))return ['profile_local_settings_save','profile_apply','profile_game_launch','profile_stop','profile_recover','profile_reset','profile_selection_mode','profile_game_rule_save','profile_game_rule_remove','profile_game_preference_save','profile_game_preference_remove','profile_input_learn_cancel','profile_desktop_stop','profile_desktop_recover','profile_desktop_configure','profile_retired_recovery_recover','profile_retired_recovery_acknowledge','profile_retired_recovery_provision_recover','profile_retired_recovery_provision_cancel'].includes(command);
  if(/^(module_usb_|controller_|game_|companion_)/.test(command))return !['module_usb_ports','module_usb_list','module_usb_status','module_usb_config_read','module_usb_actions_describe','module_usb_actions_state','module_usb_events_state','module_usb_events_read','controller_list','controller_inspect','controller_visibility_status','controller_visibility_inspect','controller_visibility_processes','game_get_state','game_telemetry_inspect','game_artwork_list','game_artwork_preview','companion_status','companion_inventory','companion_firmware_check','companion_usb_ports','companion_usb_status','companion_browser_certificate'].includes(command);
  return ['start','stop','apply','save','register','unregister','enable','disable','forget','update-profile','configure','claim','release','connect','disconnect','reconnect','remove','config_apply','config_apply_save','config_save','config_discard','config_reset','action_claim','action_renew','action_release','action_invoke','action_invoke_owned','debug_invoke'].includes(command);
}
function queuedRequest(request: GatewayRequest, operation: () => Promise<GatewayState>): Promise<GatewayState> {
  if(request.command==='profile_retired_recovery_provision_cancel')retiredRecoveryEpoch={};
  const retiredStart=request.command==='profile_retired_recovery_provision_recover',retiredEpoch=retiredRecoveryEpoch;
  if(learningInterrupt(request))learningEpoch={};
  const learningStart=request.command==='profile_input_learn_start',capturedLearningEpoch=learningEpoch;
  const profileApply = request.command === 'profile_apply' || request.command === 'profile_game_launch';
  const profileControl = ['profile_stop', 'profile_recover','profile_desktop_stop','profile_desktop_recover'].includes(request.command);
  const profileInterrupt = ['profile_stop', 'profile_recover','profile_desktop_stop','profile_desktop_recover','profile_desktop_configure', 'profile_local_settings_save', 'controller_visibility_restore'].includes(request.command);
  if (profileInterrupt) profileApplyEpoch = {};
  const applyEpoch = profileApplyEpoch;
  const usb = request.command.startsWith('module_usb_') && request.command !== 'module_usb_ports' && request.command !== 'module_usb_list';
  const usbEndpoint = request.key ?? request.port ?? 'invalid';
  let usbTicket: { epoch: number; pending: number } | undefined;
  if (usb) {
    usbTicket = usbRoutes.get(usbEndpoint) ?? { epoch: 0, pending: 0 };
    usbRoutes.set(usbEndpoint, usbTicket); usbTicket.pending++;
    // Finish only the operation already handed to native. Cancel older queued
    // operations before Disconnect reaches the same worker; never replay them.
    if (request.command === 'module_usb_disconnect') usbTicket.epoch++;
  }
  const usbEpoch = usbTicket?.epoch;
  const key = request.command.startsWith('activity_') ? 'activity' : request.command.startsWith('widget_') ? 'widget-library' : request.command === 'profile_activation_cancel' ? 'profile-activation-cancel' : ['profile_activation_current','profile_activation_status'].includes(request.command) ? 'profile-activation-progress' : ['companion_firmware_check','companion_usb_flash'].includes(request.command) ? 'companion-firmware' : request.command.startsWith('profile_retired_recovery_') ? (request.command==='profile_retired_recovery_status'?'retired-status':request.command==='profile_retired_recovery_provision_cancel'?'retired-cancel':'retired-recovery') : request.command.startsWith('profile_input_learn_')?`profile-learning:${request.command.slice('profile_input_learn_'.length)}`:profileControl ? 'profile-control' : profileApply ? 'profile-apply' : request.command === 'profile_automatic_status' ? 'profile-automatic-status' : request.command === 'profile_desktop_status' ? 'desktop-status' : request.command === 'profile_status' ? 'profile-status' : request.command === 'profile_runtime_topology' ? 'profile-topology' : request.command === 'profile_runtime_inspect' ? 'profile-inspection' : usb ? `module-usb:${usbEndpoint}` : request.command.startsWith('module_usb_') ? 'module-usb-inventory' : request.command.startsWith('profile_companion_') ? 'profile-companion-authoring' : request.command.startsWith('profile_resources_') ? 'profile-resources' : request.command.startsWith('profile_') ? 'profiles' : request.command.startsWith('companion_') ? 'companions' : request.command === 'controller_visibility_restore' ? 'visibility-stop' : request.command.startsWith('controller_') ? 'controllers' : request.command==='game_telemetry_inspect'?'game-telemetry':request.command.startsWith('game_') ? 'games' : request.gatewayId ?? 'inventory';
  const previous = requestQueues.get(key) ?? Promise.resolve();
  const before = retiredStart?Promise.all([previous,requestQueues.get('retired-cancel')]):learningStart?Promise.all([previous,requestQueues.get('profile-learning:cancel')]):profileApply ? Promise.all([previous, requestQueues.get('profiles'), profileControlBarrier]) : previous;
  const pending = before.catch(() => {}).then(() => {
    if(retiredStart&&retiredEpoch!==retiredRecoveryEpoch)throw new GatewayError('Retained setup recovery was cancelled before dispatch; refresh its exact native record and request explicitly again.');
    if (usbTicket && request.command !== 'module_usb_disconnect' && usbEpoch !== usbTicket.epoch) throw new GatewayError('USB request was cancelled by Disconnect; refresh and request it explicitly again.');
    if (profileApply && applyEpoch !== profileApplyEpoch) throw new GatewayError(request.command === 'profile_game_launch' ? 'Profile game launch was cancelled by deactivation or recovery; launch explicitly again.' : 'Profile activation was cancelled by deactivation or recovery; activate explicitly again.');
    if(learningStart&&capturedLearningEpoch!==learningEpoch)throw new GatewayError('Input learning Start was cancelled by a native control request; start explicitly again.');
    return operation();
  });
  const settled = pending.then(() => {}, () => {});
  if (profileInterrupt) profileControlBarrier = Promise.all([profileControlBarrier, settled]).then(() => {});
  requestQueues.set(key, settled);
  void settled.then(() => {
    if (requestQueues.get(key) === settled) requestQueues.delete(key);
    if (usbTicket && --usbTicket.pending === 0 && usbRoutes.get(usbEndpoint) === usbTicket) usbRoutes.delete(usbEndpoint);
  });
  return pending;
}

export class GatewayError extends Error {
  constructor(message: string, public state?: GatewayState) { super(message); }
}
function resultState(result: GatewayResult): GatewayState {
  if (result.state?.games?.catalogue) updateGameCatalogue(result.state.games.catalogue);
  if (!result.ok || !result.state) throw new GatewayError(result.error ?? 'The gateway request failed.', result.state);
  return result.state;
}

export type ProfileCleanupEvent={type:'start'|'finish';activationId:string;phase:'deactivating'|'recovering';error?:string};
const cleanupListeners=new Set<(event:ProfileCleanupEvent)=>void>();
export function subscribeProfileCleanup(listener:(event:ProfileCleanupEvent)=>void):()=>void {
  cleanupListeners.add(listener);return()=>{cleanupListeners.delete(listener);};
}
function publishCleanup(event:ProfileCleanupEvent){
  for(const listener of cleanupListeners){try{listener(event);}catch{/* Progress rendering never delays emergency output cleanup. */}}
}

export function gatewayRequest(request: GatewayRequest): Promise<GatewayState> {
  if (/^(mapping_|dcs_|integration_)/.test(request.command) || ['companion_feedback','companion_feedback_routes','companion_game_capture','controller_visibility_apply','controller_visibility_launch'].includes(request.command)) return Promise.reject(new GatewayError('This standalone route is retired. Use Profiles for authoring and activation, or retained recovery for previous ownership records.'));
  return queuedRequest(request, async () => {
    if (isTauri()) {
      const phase=request.command==='profile_stop'?'deactivating':request.command==='profile_recover'?'recovering':undefined;
      const activationId=phase&&cleanupListeners.size?(request.activationId??crypto.randomUUID()):undefined;
      let failure:string|undefined;
      if(activationId&&phase)publishCleanup({type:'start',activationId,phase});
      try {
        const result=await invoke<GatewayResult>('gateway_request', { request: { ...request,...(activationId?{activationId}:{}), clientId } });
        return resultState(result);
      } catch (error) {
        const wrapped=error instanceof GatewayError?error:new GatewayError(error instanceof Error ? error.message : String(error));
        failure=wrapped.message;throw wrapped;
      } finally {
        if(activationId&&phase)publishCleanup({type:'finish',activationId,phase,...(failure?{error:failure}:{})});
      }
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
