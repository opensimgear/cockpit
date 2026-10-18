import type { GatewayRequest, GatewayState } from './gateway';
import type { GatewayEntry } from './gateway-workspace';
import type { HostController } from './controllers';
import type { UsbModuleSummary } from './module-usb';
import { ProfileInventoryClient, type ResourceSummary } from './profile-inventory.ts';
import { canChangeController } from './controller-lifecycle.ts';
import { gameIntegrationLabel, type GameId } from './games.ts';
import { SystemInventoryRegistry, type InventoryStorage, type SystemObservation, type SystemComponent, type SystemSnapshot, type SystemTarget } from './system-inventory.ts';

export type NativeSystemInventory = {
  observedAt?: number;
  offset?: number; total?: number; nextOffset?: number | null;
  virtualDevices: { id: string; name: string; available: boolean | null; status: string; createdByCockpit: boolean | null; owner?: string | null; ownerPid?: number | null; diagnostics: string[] }[];
  hiddenDevices: { id: string; name: string; instanceId: string; present: boolean | null; hidden: boolean | null; configured?: boolean; inverse?: boolean; scope?: string; controllerId?: string | null; controllerIds?: string[]; interfacePaths?: string[]; ancestors?: string[]; diagnostics: string[] }[];
  diagnostics: string[];
};
type Send = (request: GatewayRequest) => Promise<GatewayState>;
type Capture = { provider: string; observations: SystemObservation[]; diagnostics?: string[] };
const message = (error: unknown) => error instanceof Error ? error.message : String(error);
const string = (value: unknown, fallback = '') => typeof value === 'string' ? value : fallback;
const record = (value: unknown): Record<string, unknown> => value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
const rows = (value: unknown): Record<string, unknown>[] => Array.isArray(value) ? value.map(record) : [];
const target = (kind: SystemTarget['kind'], id: string, gatewayId?: string): SystemTarget => ({ kind, id, ...(gatewayId ? { gatewayId } : {}) });
const localEffect = 'Forget this local remembered record. User configuration and bindings remain intact and may show an unresolved target. Automatically discovered components can reappear when observed again.';
function observation(id: string, name: string, type: SystemObservation['type'], provider: string, now: number, extra: Partial<SystemObservation> = {}): SystemObservation {
  return { id, name, type, role: '', provider, observedAt: now, stale: false, presence: 'unknown', hidden: null, enabled: null, owner: null, capture: 'unknown', status: '', diagnostics: [], connections: [], targets: [], evidence: [], references: [], forget: { allowed: false, reason: 'Current presence or removal capability is unknown.', effect: localEffect }, ...extra };
}
function link(id: string, from: string, transport: 'usb' | 'can' | 'network' | 'software' | 'unknown', current: boolean, endpoint?: string) {
  return { id: `${transport}:${from}:${id}:${endpoint ?? ''}`, from, to: id, transport, current, ...(endpoint ? { endpoint } : {}) };
}
export function gatewayObservations(entries: readonly GatewayEntry[], now: number): SystemObservation[] {
  return entries.flatMap(entry => {
    const id = `gateway:${entry.id}`, state = entry.state, connected = !!state?.connected;
    const gateway = observation(id, entry.port.backend === 'stm32' ? 'OpenSimGear CAN gateway' : 'Pico W USB gateway', 'gateway', 'gateways', now, {
      presence: connected ? 'connected' : entry.present ? 'unavailable' : 'offline',
      role: entry.port.serialNumber ?? entry.id, status: state?.reconnecting ? 'Reconnecting' : entry.busy ? 'Working' : connected && state?.running ? 'USB HID active' : '',
      connections: [link(id, 'host', 'usb', entry.present, state?.port ?? entry.port.path)], targets: [target('gateway', entry.id)],
      evidence: [entry.id], diagnostics: entry.error ? [entry.error] : [],
      forget: { allowed: !entry.present && !connected && !state?.claimedModules?.length, reason: 'Disconnect the gateway and release its owned modules before forgetting its local observations.', effect: 'Forget local gateway and stale child-route observations. An unavailable gateway cannot confirm removal from its persistent storage. Independently observed USB connections and user bindings remain intact.' },
    });
    return [gateway, ...(state?.modules ?? []).map(module => {
      const key = `${entry.id}/${module.id}`, id = `module:${key}`, online = connected && module.online;
      const owned = !!state?.claimedModules?.includes(module.id);
      return observation(id, module.settings?.label || module.name, 'osg', 'gateways', now, {
        role: module.outputOnly ? 'Output device' : module.kind, presence: online ? 'connected' : connected || !entry.present ? 'offline' : 'unknown', enabled: module.enabled ?? module.registered,
        owner: owned ? 'Setup session' : module.directUsb ? 'Direct USB input owner' : null,
        capture: module.outputOnly ? 'unavailable' : online && module.active ? 'ready' : 'waiting',
        status: module.directUsb ? 'Direct USB owns input (local attachment unconfirmed)' : module.published === true && !online ? 'Neutral controller retained' : '',
        connections: [link(id, gateway.id, entry.port.backend === 'stm32' ? 'can' : 'software', online)],
        targets: [target('module', module.id, entry.id)], evidence: [`route:${entry.id}/${module.id}`, ...(module.boot ? [`boot:${module.boot}`] : [])],
        forget: { allowed: !entry.busy && canChangeController(state, module, 'forget'), reason: online ? 'Device is connected.' : owned ? 'Release the active session first.' : 'Gateway removal is unavailable; only eligible offline records can be removed.', effect: connected ? 'Forget this offline module from the gateway persistent inventory. Published controller layout may change and games may require rebinding. Module settings and user bindings are retained.' : localEffect },
      });
    })];
  });
}
/** Reuse verified native correlation; never guess identity from display strings or VID/PID. */
export function correlateSystem(observations: SystemObservation[], resources: readonly ResourceSummary[]): SystemObservation[] {
  const native = new Map<string, string>();
  for (const r of resources) if (r.resourceType === 'osg-module') for (const c of r.connections) {
    const id = c.scope === 'standalone-usb' ? `usb:${c.key}` : c.scope === 'host-controllers' ? `controller:${c.key}` : c.scope.startsWith('gateway:') ? `module:${c.scope.slice(8)}/${c.key}` : '';
    if (id) native.set(id, r.id);
  }
  const groups = new Map<string, SystemObservation[]>();
  for (const o of observations) { const id = native.has(o.id) ? `resource:${native.get(o.id)}` : o.id; groups.set(id, [...(groups.get(id) ?? []), o]); }
  const result = [...groups].map(([id, group]) => {
    const preferred = group.find(c => c.targets.some(t => t.kind === 'module')) ?? group.find(c => c.type === 'osg') ?? group[0];
    const connected = group.some(c => c.presence === 'connected');
    return { ...preferred, id, presence: connected ? 'connected' as const : preferred.presence,
      hidden: group.some(c => c.hidden === true) ? true : group.some(c => c.hidden === false) ? false : null,
      owner: group.find(c => c.owner)?.owner ?? null,
      capture: group.some(c => c.capture === 'ready') ? 'ready' as const : preferred.capture,
      connections: group.flatMap(c => c.connections.map(x => ({ ...x, to: id }))),
      targets: group.flatMap(c => c.targets), evidence: [...new Set(group.flatMap(c => c.evidence))], diagnostics: [...new Set(group.flatMap(c => c.diagnostics))],
      forget: connected || group.some(c => c.owner) ? { ...preferred.forget, allowed: false, reason: 'This component is connected or owned.' } : preferred.forget,
    };
  });
  return result;
}

export class SystemInventoryController {
  readonly registry: SystemInventoryRegistry;
  private send: Send;
  private gateways: () => readonly GatewayEntry[];
  private pending: Promise<SystemSnapshot> | null = null;
  private epoch = 0;
  private disposed = false;
  private captures = new Map<string, Capture>();
  private resources: ResourceSummary[] = [];
  private onChange: (snapshot: SystemSnapshot) => void;
  constructor(options: { request: Send; gateways: () => readonly GatewayEntry[]; storage?: InventoryStorage; onChange?: (snapshot: SystemSnapshot) => void }) {
    this.send = options.request; this.gateways = options.gateways; this.registry = new SystemInventoryRegistry(options.storage); this.onChange = options.onChange ?? (() => {});
  }
  get snapshot() { return this.registry.snapshot; }
  refresh(): Promise<SystemSnapshot> {
    if (this.disposed) return Promise.resolve(this.snapshot);
    if (this.pending) return this.pending;
    const epoch = this.epoch;
    const pending = this.collect(epoch).finally(() => { if (this.pending === pending) this.pending = null; });
    this.pending = pending; return pending;
  }
  private async collect(epoch: number): Promise<SystemSnapshot> {
    const now = Date.now(), diagnostics: string[] = [], complete: string[] = ['host', 'gateways'];
    const providers = [this.controllers(now), this.usb(now), this.companions(now), this.integrations(now), this.native(now)];
    const results = await Promise.allSettled(providers);
    if (this.disposed || epoch !== this.epoch) return this.snapshot;
    const names = ['controllers', 'usb', 'companions', 'integrations', 'native'];
    results.forEach((result, index) => {
      if (result.status === 'fulfilled') { this.captures.set(names[index], result.value); if (names[index] !== 'native' || !result.value.diagnostics?.length) complete.push(names[index]); diagnostics.push(...(result.value.diagnostics ?? [])); }
      else { this.captures.delete(names[index]); diagnostics.push(`${names[index]}: ${message(result.reason)}`); }
    });
    // The common resource facade already proves USB/CAN/host HID equivalence.
    try {
      const client = new ProfileInventoryClient(async request => { const value = (await this.send(request)).profileResources; if (!value) throw Error('Shared native identity evidence unavailable.'); return value; });
      this.resources = JSON.parse(JSON.stringify((await client.listAll()).resources));
    } catch (error) { diagnostics.push(`Identity correlation is stale: ${message(error)}`); }
    if (this.disposed || epoch !== this.epoch) return this.snapshot;
    let observations = [observation('host', 'Your computer', 'host', 'host', now, { presence: 'connected', role: 'Host', forget: { allowed: false, reason: 'The host cannot be forgotten.', effect: '' } }), ...gatewayObservations(this.gateways(), now), ...[...this.captures.values()].flatMap(c => c.observations)];
    // Resolve HidHide PnP collections to the original controller, not duplicate nodes.
    const hidden = this.captures.get('native')?.observations.filter(c => c.targets.some(t => t.kind === 'visibility')) ?? [];
    const consumed = new Set<string>();
    for (const h of hidden) for (const source of observations.filter(c => c.targets.some(t => t.kind === 'controller'))) if (source.targets.some(t => h.evidence.includes(`controller:${t.id}`))) {
      source.hidden = h.hidden; source.diagnostics.push(...h.diagnostics); source.evidence.push(...h.evidence);
      if (h.presence !== 'unknown') source.presence = h.presence;
      if (h.hidden || h.presence === 'connected') source.forget.allowed = false;
      consumed.add(h.id);
    }
    observations = observations.filter(c => !consumed.has(c.id));
    observations = correlateSystem(observations, this.resources);
    const snapshot = this.registry.update(observations, complete, diagnostics, now); this.onChange(snapshot); return snapshot;
  }
  private async controllers(now: number): Promise<Capture> {
    const sources: HostController[] = []; let offset = 0, session: string | undefined, total: number | undefined;
    do {
      const page = (await this.send({ command: 'controller_list', offset, limit: 32 })).controllers;
      if (!page?.sources || page.sources.length > 32 || offset + page.sources.length > page.total || page.offset !== offset || !Number.isSafeInteger(page.total) || page.total < 0 || page.total > 4096 || (offset < page.total && !page.sources.length) || (session !== undefined && page.session !== session) || (total !== undefined && total !== page.total)) throw Error('Controller inventory changed or returned an incomplete page.');
      session = page.session; total = page.total; sources.push(...page.sources); offset += page.sources.length;
    } while (offset < total!);
    return { provider: 'controllers', observations: sources.map(s => {
      const id = `controller:${s.id}`;
      return observation(id, s.name, s.osgHid ? 'osg' : 'controller', 'controllers', now, { role: s.role, presence: s.online ? 'connected' : 'offline', capture: s.online ? s.baselineReady ? 'ready' : 'waiting' : 'unavailable',
        status: `${s.controlCount} controls`, evidence: [`controller:${s.id}`, `identity:${s.identityConfidence}`], targets: [target('controller', s.id)],
        connections: [link(id, 'host', 'unknown', s.online)],
        forget: { allowed: !s.online, reason: 'Disconnect this controller before forgetting it.', effect: localEffect } });
    }) };
  }
  private async usb(now: number): Promise<Capture> {
    const modules: UsbModuleSummary[] = []; let offset = 0, total: number | undefined;
    do {
      const page = (await this.send({ command: 'module_usb_list', offset, limit: 32 })).moduleUsb;
      if (!page || !('modules' in page) || page.modules.length > 32 || offset + page.modules.length > page.total || page.offset !== offset || !Number.isSafeInteger(page.total) || page.total < 0 || page.total > 128 || (offset < page.total && !page.modules.length) || total !== undefined && total !== page.total) throw Error('USB inventory changed or returned an incomplete page.');
      total = page.total; modules.push(...page.modules); offset += page.modules.length;
    } while (offset < total!);
    return { provider: 'usb', observations: modules.map(s => {
      const id = `usb:${s.key}`, snapshot = s.snapshot;
      return observation(id, snapshot.identity?.profile ?? s.port.friendlyName ?? 'OSG USB endpoint', 'osg', 'usb', now, { role: snapshot.identity?.board ?? 'USB module', presence: s.present ? snapshot.connected ? 'connected' : 'unavailable' : 'offline',
        owner: snapshot.claimedBy, targets: [target('usb', s.key)], connections: [link(id, 'host', 'usb', s.present, s.port.path)],
        evidence: [s.key, ...(snapshot.identity ? [`verified:${snapshot.identity.moduleId}:${snapshot.identity.serial}`] : [])], diagnostics: [...s.diagnostics, ...(snapshot.error ? [snapshot.error] : [])],
        forget: { allowed: !s.present && !snapshot.claimedBy, reason: 'Disconnect the module and release its active claim first.', effect: localEffect } });
    }) };
  }
  private async companions(now: number): Promise<Capture> {
    const state = (await this.send({ command: 'companion_status' })).companions;
    if (!state?.host) throw Error('Companion inventory unavailable.');
    const observations: SystemObservation[] = [];
    for (const c of rows(state.host.clients)) {
      const key = string(c.companionId); if (!key) continue; const id = `companion:${key}`;
      const online = c.connected === true || c.online === true || !!c.sessionId;
      observations.push(observation(id, string(c.name, key), 'companion', 'companions', now, { role: string(c.kind, 'Installation'), presence: online ? 'connected' : 'offline',
        status: 'Paired installation', targets: [target('companion', key)], evidence: [`companion:${key}`],
        connections: online ? [link(id, 'host', 'network', true, string(c.peer, string(c.address)) || undefined)] : [],
        forget: { allowed: !online, reason: 'Disconnect the companion before revoking it.', effect: 'Revoke this companion installation and its assignments. Re-pairing will be required. Saved panel definitions and user bindings remain intact.' } }));
    }
    for (const instance of rows(state.host.instances)) {
      const key = string(instance.instanceId); if (!key) continue; const id = `companion-instance:${key}`, client = string(instance.companionId);
      const connected = instance.connected === true, armed = instance.armed === true;
      observations.push(observation(id, string(instance.name, key), 'companion', 'companions', now, { role: 'Assigned controller instance', presence: connected ? 'connected' : 'offline', owner: connected && instance.sessionId ? string(instance.sessionId) : null,
        capture: connected && armed ? 'ready' : connected ? 'waiting' : 'unavailable',
        connections: client ? [link(id, `companion:${client}`, 'software', connected)] : [], targets: [target('companion', client || key)], evidence: [`instance:${key}`],
        status: armed && connected ? 'Input session active' : 'Not armed', forget: { allowed: false, reason: 'Manage assignments in Companions; revoke an installation explicitly.', effect: '' } }));
    }
    for (const usb of state.usb?.devices ?? (state.usb ? [state.usb] : [])) {
      const key = usb.device?.companionId, existing = observations.find(c => c.id === `companion:${key}`);
      if (!usb.port) continue;
      const id = existing?.id ?? `companion-usb:${usb.port}`;
      const edge = link(id, 'host', 'usb', usb.connected, usb.port);
      if (existing) { existing.connections.push(edge); if (usb.connected) existing.presence = 'connected'; }
      else observations.push(observation(id, usb.device?.name ?? 'USB companion', 'companion', 'companions', now, { role: usb.device?.boardProfile ?? 'ESP32', presence: usb.connected ? 'connected' : 'offline', connections: [edge], targets: key ? [target('companion', key)] : [], evidence: [usb.port], status: usb.stage ?? '' }));
    }
    return { provider: 'companions', observations, diagnostics: state.listener?.lastError ? [state.listener.lastError] : [] };
  }
  private async integrations(now: number): Promise<Capture> {
    const state = (await this.send({ command: 'game_get_state' })).games;
    if (!state?.catalogue) throw Error('Integration inventory unavailable.');
    return { provider: 'integrations', observations: state.catalogue.map(game => {
      const id = `integration:${game.id}`, setup = game.integrationSetup;
      const receiving = setup?.live === 'receiving';
      const endpoints = Object.entries(setup?.connection ?? {}).filter(([key, value]) => /host|address|port|endpoint|source/i.test(key) && ['string', 'number'].includes(typeof value)).map(([key, value]) => `${key}: ${value}`).join(', ');
      return observation(id, `${game.name} · ${game.provider}`, 'integration', 'integrations', now, { role: game.provider, presence: receiving ? 'connected' : !game.installed || setup?.state === 'unsupported-platform' ? 'unavailable' : 'unknown',
        status: gameIntegrationLabel(game), diagnostics: [setup?.message, setup?.liveMessage].filter((x): x is string => !!x), evidence: [`integration:${game.id}`], targets: [target('integration', game.id)],
        connections: [link(id, 'host', 'software', receiving, endpoints || undefined)],
        forget: { allowed: false, reason: 'This integration is in the supported catalogue. Manage installation and settings in game details.', effect: '' } });
    }), diagnostics: state.error ? [state.error] : [] };
  }
  private async native(now: number): Promise<Capture> {
    const state = (await this.send({ command: 'system_inventory', offset: 0, limit: 32 })).systemInventory;
    if (!state || !Array.isArray(state.virtualDevices) || !Array.isArray(state.hiddenDevices)) throw Error('Native system inspection unavailable. Restart the updated service.');
    const hiddenDevices = [...state.hiddenDevices]; let nextOffset = state.nextOffset;
    while (nextOffset != null) {
      if (!Number.isSafeInteger(nextOffset) || nextOffset !== hiddenDevices.length || nextOffset > 4096) throw Error('Native inspection returned an invalid page.');
      const page = (await this.send({ command: 'system_inventory', offset: nextOffset, limit: 32, observedAt: state.observedAt })).systemInventory;
      if (!page || page.observedAt !== state.observedAt || page.total !== state.total || page.offset !== nextOffset || !page.hiddenDevices?.length || nextOffset + page.hiddenDevices.length > page.total!) throw Error('Native system inventory changed during inspection. Refresh again.');
      hiddenDevices.push(...page.hiddenDevices); nextOffset = page.nextOffset;
    }
    return { provider: 'native', diagnostics: state.diagnostics, observations: [
      ...state.virtualDevices.map(v => {
        const id = `virtual:${v.id}`, owned = v.owner ?? (v.ownerPid ? `PID ${v.ownerPid}` : null);
        return observation(id, v.name, 'virtual', 'native', now, { role: 'vJoy', presence: v.available === true ? 'connected' : v.available === false ? 'unavailable' : 'unknown', owner: owned, status: `${v.status}${v.createdByCockpit === true ? ' · Created by Cockpit' : ' · Creation provenance unknown'}`, diagnostics: v.diagnostics, evidence: [v.id], targets: [target('virtual', v.id)], connections: [link(id, 'host', 'software', v.available === true)], forget: { allowed: v.available === false && !owned, reason: 'Available or owned virtual devices cannot be forgotten.', effect: 'Forget only local virtual-device inventory metadata. The OS device, vJoy layout, driver and bindings are retained.' } });
      }),
      ...hiddenDevices.map(h => {
        const id = `visibility:${h.id}`;
        return observation(id, h.name, 'controller', 'native', now, { role: 'Physical device', presence: h.present === true ? 'connected' : h.present === false ? 'offline' : 'unknown', hidden: h.hidden,
          status: h.configured ? 'Configured visibility rule' : '', diagnostics: [...h.diagnostics, ...(h.inverse ? ['HidHide inverse application policy is active.'] : [])],
          evidence: [h.instanceId, ...(h.controllerIds ?? (h.controllerId ? [h.controllerId] : [])).map(x => `controller:${x}`)], targets: [target('visibility', h.instanceId)],
          forget: { allowed: false, reason: 'Manage visibility rules explicitly. Inventory cleanup cannot remove HidHide configuration.', effect: '' } });
      }),
    ] };
  }
  /** Inspect existing local bindings for confirmation only; never change or activate them. */
  async reviewForget(ids: string[]): Promise<SystemComponent[]> {
    const selected = this.snapshot.components.filter(c => ids.includes(c.id));
    const result = selected.map(c => ({ ...c, references: [...c.references] }));
    try {
      const profiles = (await this.send({ command: 'profile_list' })).profiles?.profiles;
      if (!profiles || profiles.length > 256) throw Error('Stored binding inventory unavailable.');
      for (const profile of profiles) {
        const assignments = (await this.send({ command: 'profile_assignments_get', id: profile.id })).profiles?.assignments;
        if (!assignments) throw Error(`Stored references for ${profile.name} unavailable.`);
        for (const c of result) {
          const sourceIds = new Set([c.id, c.id.replace(/^resource:/, ''), ...c.targets.map(t => t.id)]);
          for (const a of assignments) if (sourceIds.has(a.sourceId)) c.references.push({ kind: 'binding', id: `${profile.id}/${a.resourceId}`, name: `${profile.name} / ${a.resourceId}` });
        }
      }
    } catch (error) { for (const c of result) c.diagnostics = [...c.diagnostics, `Affected stored references could not be fully inspected: ${message(error)} All user bindings are retained and may show unresolved targets.`]; }
    return result;
  }
  /** Refresh and revalidate immediately before a one-shot guarded provider mutation. */
  async forget(id: string): Promise<void> {
    await this.refresh();
    const c = this.snapshot.components.find(c => c.id === id);
    if (!c?.forget.allowed || c.presence === 'connected' || c.owner || c.hidden) throw Error(c?.forget.reason || 'The selected component is no longer eligible.');
    const t = c.targets.find(t => t.kind === 'controller' || t.kind === 'module' || t.kind === 'companion');
    this.epoch++;
    if (t?.kind === 'controller') await this.send({ command: 'controller_forget', sourceId: t.id });
    else if (t?.kind === 'module' && this.gateways().some(e => e.id === t.gatewayId && e.state?.connected)) await this.send({ command: 'forget', gatewayId: t.gatewayId, moduleId: t.id });
    else if (t?.kind === 'companion') await this.send({ command: 'companion_revoke', companion: { companionId: t.id } });
    this.registry.forget(id); this.onChange(this.snapshot);
  }
  dispose() { this.disposed = true; this.epoch++; }
}
export const integrationGameId = (component: SystemComponent): GameId | undefined => component.targets.find(t => t.kind === 'integration')?.id as GameId | undefined;
