import type { GatewayRequest, GatewayState } from './gateway';
import type { GatewayEntry } from './gateway-workspace';
import type { HostController } from './controllers';
import type { UsbModuleSummary } from './module-usb';
import { ProfileTopologyClient } from './profile-activity';
import { ProfileInventoryClient, type ResourceSummary } from './profile-inventory.ts';
import { canChangeController } from './controller-lifecycle.ts';
import { gameIntegrationLabel, type GameId } from './games.ts';
import { SystemInventoryRegistry, type InventoryStorage, type SystemObservation, type SystemComponent, type SystemSnapshot, type SystemTarget } from './system-inventory.ts';

export type NativeSystemInventory = {
  observedAt?: number;
  visibilityInspected?: boolean;
  offset?: number; total?: number; nextOffset?: number | null;
  controllerAssociations?: { controllerId: string; gatewayId?: string; moduleId?: string; usbKey?: string; evidence: string[] }[];
  virtualDevices: { id: string; name: string; available: boolean | null; status: string; createdByCockpit: boolean | null; owner?: string | null; ownerPid?: number | null; diagnostics: string[] }[];
  hiddenDevices: { id: string; name: string; instanceId: string; present: boolean | null; hidden: boolean | null; configured?: boolean | null; inverse?: boolean | null; scope?: string; controllerId?: string | null; controllerIds?: string[]; interfacePaths?: string[]; parentInstanceId?: string | null; containerId?: string | null; ancestors?: string[]; diagnostics: string[] }[];
  diagnostics: string[];
};
type Send = (request: GatewayRequest) => Promise<GatewayState>;
type Capture = { provider: string; observations: SystemObservation[]; complete?: boolean; visibilityInspected?: boolean; diagnostics?: string[]; associations?: NativeSystemInventory['controllerAssociations'] };
const message = (error: unknown) => error instanceof Error ? error.message : String(error);
const string = (value: unknown, fallback = '') => typeof value === 'string' ? value : fallback;
const record = (value: unknown): Record<string, unknown> => value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
const rows = (value: unknown): Record<string, unknown>[] => Array.isArray(value) ? value.map(record) : [];
const target = (kind: SystemTarget['kind'], id: string, gatewayId?: string): SystemTarget => ({ kind, id, ...(gatewayId ? { gatewayId } : {}) });
const localEffect = 'Forget this local remembered record. User configuration and bindings remain intact and may show an unresolved target. Automatically discovered components can reappear when observed again.';
function observation(id: string, name: string, type: SystemObservation['type'], provider: string, now: number, extra: Partial<SystemObservation> = {}): SystemObservation {
  return { id, name, type, role: '', provider, observedAt: now, stale: false, presence: 'unknown', hidden: null, enabled: null, owner: null, capture: 'unknown', status: '', diagnostics: [], connections: [], targets: [], evidence: [], references: [], forget: { allowed: false, reason: 'Current presence or removal capability is unknown.', effect: localEffect }, ...extra };
}
function link(id: string, from: string, transport: 'usb' | 'can' | 'network' | 'bluetooth' | 'software' | 'unknown', current: boolean, endpoint?: string) {
  return { id: `${transport}:${from}:${id}:${endpoint ?? ''}`, from, to: id, transport, current, ...(endpoint ? { endpoint } : {}) };
}
export function gatewayObservations(entries: readonly GatewayEntry[], now: number): SystemObservation[] {
  return entries.flatMap(entry => {
    const id = `gateway:${entry.id}`, state = entry.state, connected = !!state?.connected;
    const gateway = observation(id, entry.port.backend === 'stm32' ? 'OpenSimGear CAN gateway' : 'Pico W USB gateway', 'gateway', 'gateways', now, {
      presence: connected ? 'connected' : entry.present ? 'unavailable' : 'offline',
      role: entry.port.serialNumber ?? entry.id, status: entry.autoConnectPaused ? 'Connection paused' : state?.reconnecting ? 'Reconnecting' : entry.busy ? 'Working' : connected && state?.running ? 'USB HID active' : '',
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
        inputOwner: online && !module.outputOnly ? module.directUsb ? 'Device USB' : module.active ? `Gateway ${entry.id}` : null : null,
        configurationOwner: owned ? 'Setup session' : null,
        configurationRoute: owned ? `${entry.port.backend === 'stm32' ? 'CAN' : 'Gateway'} · ${entry.id}/${module.id}` : null,
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
  const bindings = new Map<string, string[]>();
  for (const r of resources) for (const c of r.connections) {
    const id = c.scope === 'standalone-usb' ? `usb:${c.key}` : c.scope === 'host-controllers' ? `controller:${c.key}` : c.scope === 'companions' ? `companion-instance:${c.key}` : c.scope.startsWith('gateway:') ? `module:${c.scope.slice(8)}/${c.key}` : '';
    if (id) {
      bindings.set(id, [...(bindings.get(id) ?? []), `binding-source:${r.id}`]);
      if (r.resourceType === 'osg-module') native.set(id, r.id);
    }
  }
  const groups = new Map<string, SystemObservation[]>();
  for (const o of observations) { const id = native.has(o.id) ? `resource:${native.get(o.id)}` : o.id; groups.set(id, [...(groups.get(id) ?? []), o]); }
  const result = [...groups].map(([id, group]) => {
    const preferred = group.find(c => c.targets.some(t => t.kind === 'module')) ?? group.find(c => c.type === 'osg') ?? group[0];
    const connected = group.some(c => c.presence === 'connected');
    return { ...preferred, id, presence: connected ? 'connected' as const : preferred.presence,
      hidden: group.some(c => c.hidden === true) ? true : group.some(c => c.hidden === false) ? false : null,
      owner: group.find(c => c.owner)?.owner ?? null,
      inputOwner: group.find(c => c.inputOwner === 'Device USB')?.inputOwner ?? group.find(c => c.inputOwner)?.inputOwner ?? null,
      configurationOwner: group.find(c => c.configurationOwner)?.configurationOwner ?? null,
      configurationRoute: group.find(c => c.configurationOwner)?.configurationRoute ?? null,
      capture: group.some(c => c.capture === 'ready') ? 'ready' as const : preferred.capture,
      connections: group.flatMap(c => c.connections.map(x => ({ ...x, to: id }))),
      targets: group.flatMap(c => c.targets), evidence: [...new Set(group.flatMap(c => [...c.evidence, ...(bindings.get(c.id) ?? [])]))], diagnostics: [...new Set(group.flatMap(c => c.diagnostics))],
      forget: connected || group.some(c => c.owner) ? { ...preferred.forget, allowed: false, reason: 'This component is connected or owned.' } : preferred.forget,
    };
  });
  return result;
}

export class SystemInventoryController {
  readonly registry: SystemInventoryRegistry;
  private send: Send;
  private gateways: () => readonly GatewayEntry[];
  private gatewayError: () => string;
  private pending: Promise<SystemSnapshot> | null = null;
  private epoch = 0;
  private disposed = false;
  private captures = new Map<string, Capture>();
  private resources: ResourceSummary[] = [];
  private associations: NonNullable<NativeSystemInventory['controllerAssociations']> = [];
  private onChange: (snapshot: SystemSnapshot) => void;
  constructor(options: { request: Send; gateways: () => readonly GatewayEntry[]; gatewayError?: () => string; storage?: InventoryStorage; onChange?: (snapshot: SystemSnapshot) => void }) {
    this.send = options.request; this.gateways = options.gateways; this.registry = new SystemInventoryRegistry(options.storage); this.onChange = options.onChange ?? (() => {});
    this.gatewayError = options.gatewayError ?? (() => '');
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
    const discoveryError = this.gatewayError();
    const now = Date.now(), diagnostics: string[] = discoveryError ? [`Gateway discovery: ${discoveryError}`] : [], complete: string[] = discoveryError ? ['host'] : ['host', 'gateways'];
    // Native PnP/transport correlation reads the controller and USB inventories.
    // Capture those first so startup does not publish their uncorrelated aliases.
    const controllers=this.controllers(now),usb=this.usb(now);
    const nativeCapture=Promise.allSettled([controllers,usb]).then(()=>this.disposed || epoch!==this.epoch ? {provider:'native',observations:[]} as Capture : this.native(now));
    const providers = [controllers,usb,this.companions(now),this.integrations(now),nativeCapture];
    const results = await Promise.allSettled(providers);
    if (this.disposed || epoch !== this.epoch) return this.snapshot;
    const names = ['controllers', 'usb', 'companions', 'integrations', 'native'];
    results.forEach((result, index) => {
      if (result.status === 'fulfilled' && new Set(result.value.observations.map(c => c.id)).size !== result.value.observations.length) { this.captures.delete(names[index]); diagnostics.push(`${names[index]}: Provider returned duplicate identities; its inspection is incomplete.`); return; }
      if (result.status === 'fulfilled') { this.captures.set(names[index], result.value); if (result.value.complete !== false && (names[index] !== 'native' || !result.value.diagnostics?.length)) complete.push(names[index]); diagnostics.push(...(result.value.diagnostics ?? [])); }
      else { this.captures.delete(names[index]); diagnostics.push(`${names[index]}: ${message(result.reason)}`); }
    });
    const native = this.captures.get('native');
    if (native?.visibilityInspected) complete.push('visibility');
    if (native?.associations) this.associations = native.associations;
    // The common resource facade already proves USB/CAN/host HID equivalence.
    try {
      const client = new ProfileInventoryClient(async request => { const value = (await this.send(request)).profileResources; if (!value) throw Error('Shared native identity evidence unavailable.'); return value; });
      this.resources = JSON.parse(JSON.stringify((await client.listAll()).resources));
    } catch (error) { diagnostics.push(`Identity correlation is stale: ${message(error)}`); }
    if (this.disposed || epoch !== this.epoch) return this.snapshot;
    let observations = [observation('host', 'Your computer', 'host', 'host', now, { presence: 'connected', role: 'Host', forget: { allowed: false, reason: 'The host cannot be forgotten.', effect: '' } }), ...gatewayObservations(this.gateways(), now), ...[...this.captures.values()].flatMap(c => c.observations)];
    // Resolve HidHide PnP collections to the original controller, not duplicate nodes.
    const hidden = this.captures.get('native')?.observations.filter(c => c.targets.some(t => t.kind === 'visibility')) ?? [];
    const visibilityIds = new Map(hidden.map(c => [c, c.id]));
    const consumed = new Set<string>();
    for (const h of hidden) {
      const identities = h.evidence.filter(e => e.startsWith('controller:')).sort();
      const sources = observations.filter(c => c !== h && c.targets.some(t => t.kind === 'controller' && identities.includes(`controller:${t.id}`)));
      if (!sources.length && identities.length) {
        const previous = this.snapshot.components.find(c => identities.includes(c.id) || c.targets.some(t => t.kind === 'controller' && identities.includes(`controller:${t.id}`)));
        h.id = previous?.id ?? identities[0]; h.name = previous?.name ?? h.name;
        if (previous) { h.type = previous.type; h.role = previous.role; }
        h.targets.push(...identities.map(id => target('controller', id.slice(11))));
        h.connections = h.connections.map(c => link(h.id, c.from, c.transport, c.current, c.endpoint));
      }
      for (const source of sources) {
      source.hidden = source.hidden === true || h.hidden === true ? true : h.hidden;
      source.diagnostics.push(...h.diagnostics); source.evidence.push(...h.evidence); source.targets.push(...h.targets.filter(t => t.kind === 'visibility'));
      if (h.connections.length) source.connections = h.connections.map(c => link(source.id, c.from, c.transport, c.current, c.endpoint));
      if (h.presence !== 'unknown') { source.presence = h.presence; source.stale = false; }
      if (h.hidden !== false || h.status === 'Configured visibility rule' || h.presence === 'connected') {
        source.forget = { ...source.forget, allowed: false, reason: h.hidden || h.status === 'Configured visibility rule' ? 'Manage visibility rules explicitly. Inventory cleanup cannot remove HidHide configuration.' : h.presence === 'connected' ? 'Device is connected.' : 'Refresh HidHide visibility before cleaning up this device.' };
      }
      consumed.add(h.id);
      }
      if (sources.length > 1) {
        sources.sort((a, b) => a.id.localeCompare(b.id)); const first = sources[0];
        for (const other of sources.slice(1)) { first.targets.push(...other.targets); first.evidence.push(...other.evidence); first.diagnostics.push(...other.diagnostics); consumed.add(other.id); }
        first.connections = [...new Map(first.connections.map(c => [c.id, c])).values()];
      }
    }
    observations = observations.filter(c => !consumed.has(c.id));
    // Native protocol serial + interface/report evidence links published HID collections
    // to their source; a published collection is a capability, not another physical wire.
    for (const a of this.associations) {
      const source = observations.find(c => c.targets.some(t => t.kind === 'controller' && t.id === a.controllerId));
      const id = a.usbKey ? `usb:${a.usbKey}` : a.gatewayId && a.moduleId ? `module:${a.gatewayId}/${a.moduleId}` : a.gatewayId ? `gateway:${a.gatewayId}` : '';
      const destination = observations.find(c => c.id === id);
      if (!source || !destination || source === destination) continue;
      destination.targets.push(...source.targets); destination.evidence.push(...source.evidence, ...a.evidence);
      destination.diagnostics.push(...source.diagnostics);
      if (source.hidden !== null) destination.hidden = source.hidden;
      if (source.capture === 'ready') destination.capture = 'ready';
      observations = observations.filter(c => c !== source);
    }
    observations = correlateSystem(observations, this.resources);
    const aliases: Record<string, string> = {};
    for (const c of observations) for (const t of c.targets) {
      const raw = t.kind === 'controller' ? `controller:${t.id}` : t.kind === 'usb' ? `usb:${t.id}` : t.kind === 'module' ? `module:${t.gatewayId}/${t.id}` : '';
      if (raw && raw !== c.id) aliases[raw] = c.id;
    }
    for (const h of hidden) {
      const controllerId = h.evidence.find(e => e.startsWith('controller:'))?.slice(11);
      const c = observations.find(c => c.targets.some(t => t.kind === 'controller' && t.id === controllerId));
      const visibility = h.targets.find(t => t.kind === 'visibility');
      if (c && visibility) aliases[`visibility:pnp:${visibility.id.toLowerCase()}`] = c.id;
      if (c) aliases[visibilityIds.get(h)!] = c.id;
    }
    const snapshot = this.registry.update(observations, complete, diagnostics, now, aliases); this.onChange(snapshot); return snapshot;
  }
  private async controllers(now: number): Promise<Capture> {
    const sources: HostController[] = []; let offset = 0, session: string | undefined, total: number | undefined, backend = 'ready', diagnostic = '';
    const accessDiagnostics = new Set<string>();
    do {
      const page = (await this.send({ command: 'controller_list', offset, limit: 32 })).controllers;
      if (!page?.sources || page.sources.length > 32 || offset + page.sources.length > page.total || page.offset !== offset || !Number.isSafeInteger(page.total) || page.total < 0 || page.total > 4096 || (offset < page.total && !page.sources.length) || (session !== undefined && page.session !== session) || (total !== undefined && total !== page.total)) throw Error('Controller inventory changed or returned an incomplete page.');
      session = page.session; total = page.total; sources.push(...page.sources); offset += page.sources.length;
      if (page.backendStatus && page.backendStatus !== 'ready') backend = page.backendStatus;
      if (page.diagnostic) diagnostic = page.diagnostic;
      if (page.selfAccess && ['unsupported', 'blocked', 'error'].includes(page.selfAccess.status)) {
        accessDiagnostics.add(`Startup HidHide access (${page.selfAccess.status}): ${page.selfAccess.message}${page.selfAccess.changed === null ? ' Its final state could not be verified.' : ''}`);
      }
    } while (offset < total!);
    const healthy = backend === 'ready';
    const accessUncertain = accessDiagnostics.size > 0;
    return { provider: 'controllers', complete: healthy && !accessUncertain, diagnostics: [...(diagnostic || !healthy ? [`Controller capture ${backend}${diagnostic ? `: ${diagnostic}` : ''}`] : []), ...accessDiagnostics], observations: sources.map(s => {
      const id = `controller:${s.id}`, uncertain = !healthy || (!s.online && accessUncertain);
      return observation(id, s.name, s.osgHid ? 'osg' : 'controller', 'controllers', now, { role: s.role, presence: uncertain ? 'unknown' : s.online ? 'connected' : 'offline', stale: uncertain, capture: healthy && s.online ? s.baselineReady ? 'ready' : 'waiting' : 'unavailable',
        status: `${s.controlCount} inputs`, diagnostics: [...accessDiagnostics], evidence: [`controller:${s.id}`, `identity:${s.identityConfidence}`], targets: [target('controller', s.id)],
        connections: [link(id, 'host', 'unknown', healthy && s.online)],
        forget: { allowed: !uncertain && !s.online, reason: uncertain ? 'Current capture availability cannot confirm this device is offline.' : 'Disconnect this controller before forgetting it.', effect: localEffect } });
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
        owner: snapshot.claimedBy, inputOwner: snapshot.connected && snapshot.identity && (snapshot.identity.buttonCount > 0 || snapshot.identity.axisCount > 0 || snapshot.identity.hatCount > 0) ? 'Device USB' : null,
        configurationOwner: snapshot.claimedBy, configurationRoute: snapshot.claimedBy ? `USB · ${s.port.path}` : null,
        targets: [target('usb', s.key)], connections: [link(id, 'host', 'usb', s.present, s.port.path)],
        evidence: [s.key, ...(snapshot.identity ? [`verified:${snapshot.identity.moduleId}:${snapshot.identity.serial}`] : [])], diagnostics: [...s.diagnostics, ...(snapshot.error ? [snapshot.error] : [])],
        forget: { allowed: !s.present && !snapshot.claimedBy, reason: 'Disconnect the module and release its active claim first.', effect: localEffect } });
    }) };
  }
  private async companions(now: number): Promise<Capture> {
    const state = (await this.send({ command: 'companion_inventory' })).companions;
    if (!state?.host) throw Error('Companion inventory unavailable.');
    const observations: SystemObservation[] = [];
    for (const c of rows(state.host.clients)) {
      const key = string(c.companionId); if (!key) continue; const id = `companion:${key}`;
      const online = c.connected === true || c.online === true || !!c.sessionId;
      const panelNames = [...new Set(rows(state.host.instances).filter(instance => instance.companionId === key).map(instance => string(instance.name)).filter(Boolean))];
      const usbSetup = (state.usb?.devices ?? (state.usb ? [state.usb] : [])).find(usb => usb.device?.companionId === key);
      const mode = usbSetup?.device?.connectionMode ?? state.firmware?.[key]?.connectionMode;
      const configuredTransport = mode === 'serial' ? 'usb' : mode === 'wifi' || c.kind === 'browser' || c.kind === 'desktop' ? 'network' : undefined;
      const rememberedConnections = (this.snapshot.components.find(previous => previous.id === id)?.connections ?? []).map(connection => ({ ...connection, current: false }));
      const observedConnections = rememberedConnections.filter(connection => connection.observed !== false && connection.transport !== 'unknown');
      const offlineConnections = configuredTransport
        ? [...observedConnections, ...(observedConnections.some(connection => connection.transport === configuredTransport) ? [] : [{ ...link(id, 'host', configuredTransport, false), observed: false }])]
        : rememberedConnections;
      const transports = [...new Set([...(Array.isArray(c.transports) ? c.transports.filter((t): t is string => t === 'network' || t === 'usb') : []), ...rows(state.host.instances).filter(instance => instance.companionId === key && instance.connected === true && ['network', 'usb'].includes(string(instance.transport))).map(instance => string(instance.transport)), ...(c.peer || c.address ? ['network'] : [])])];
      observations.push(observation(id, string(c.name, key), 'companion', 'companions', now, { role: string(c.kind, 'Installation'), presence: online ? 'connected' : 'offline',
        status: ['Paired installation', ...panelNames.map(name => `Panel: ${name}`), ...(typeof c.panelError==='string'&&c.panelError?['Panel error: '+c.panelError]:[])].join(' · '), targets: [target('companion', key)], evidence: [`companion:${key}`],
        connections: online ? (transports.length ? transports : ['unknown']).map(transport => link(id, 'host', transport as 'usb' | 'network' | 'unknown', true, transport === 'network' ? string(c.peer, string(c.address)) || undefined : undefined)) : offlineConnections,
        forget: { allowed: !online, reason: 'Disconnect the companion before revoking it.', effect: 'Revoke this companion installation and its assignments. Re-pairing will be required. Saved panel definitions and user bindings remain intact.' } }));
    }
    for (const pending of rows(state.host.pending)) {
      const key = string(pending.requestId); if (!key) continue; const id = `companion-pending:${key}`;
      observations.push(observation(id, string(pending.name, 'Pairing request'), 'companion', 'companions', now, {
        role: 'Pairing request · Unverified installation', presence: 'unknown', status: pending.approved === true ? 'Approved · Waiting for connection' : 'Waiting for pairing approval',
        evidence: [`pairing-request:${key}`], connections: pending.transport === 'usb' || pending.transport === 'network' ? [link(id, 'host', pending.transport, false)] : [],
        forget: { allowed: false, reason: 'Review or cancel this pairing request in Companions.', effect: '' },
      }));
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
      if (existing) {
        if (usb.connected) existing.connections = existing.connections.filter(c => c.transport !== 'unknown');
        existing.connections = existing.connections.filter(c => c.transport !== 'usb'); existing.connections.push(edge);
        if (usb.connected) { existing.presence = 'connected'; existing.forget.allowed = false; existing.forget.reason = 'Disconnect this USB companion before revoking its installation.'; }
      }
      else observations.push(observation(id, usb.device?.name ?? 'USB companion', 'companion', 'companions', now, { role: usb.device?.boardProfile ?? 'ESP32', presence: usb.connected ? 'connected' : 'offline', connections: [edge], targets: key ? [target('companion', key)] : [], evidence: [usb.port], status: usb.stage ?? '' }));
    }
    return { provider: 'companions', observations, diagnostics: state.listener?.lastError ? [state.listener.lastError] : [] };
  }
  private async integrations(now: number): Promise<Capture> {
    const state = (await this.send({ command: 'game_get_state' })).games;
    if (!state?.catalogue) throw Error('Integration inventory unavailable.');
    const runtime = (await this.send({ command: 'profile_status' })).profiles?.profileSession;
    let activeGame: string | undefined;
    if (runtime?.owner?.gameId && runtime.appliedSnapshot && ['running', 'degraded', 'awaiting-game'].includes(runtime.state)) {
      const topology = await new ProfileTopologyClient(async request => (await this.send(request)).profileTopology).load(runtime.owner);
      const gameNodes = new Set(topology.nodes.filter(node => node.kind === 'game').map(node => node.id));
      if (topology.edges.some(edge => gameNodes.has(edge.from.nodeId) || gameNodes.has(edge.to.nodeId))) activeGame = runtime.owner.gameId;
    }
    return { provider: 'integrations', observations: state.catalogue.filter(game => !!game.provider).map(game => {
      const id = `integration:${game.id}`, setup = game.integrationSetup;
      const receiving = setup?.live === 'receiving';
      const endpoints = Object.entries(setup?.connection ?? {}).filter(([key, value]) => /host|address|port|endpoint|source/i.test(key) && ['string', 'number'].includes(typeof value)).map(([key, value]) => `${key}: ${value}`).join(', ');
      return observation(id, `${game.name} · ${game.provider}`, 'integration', 'integrations', now, { role: game.provider ?? undefined, presence: receiving ? 'connected' : !game.installed || setup?.state === 'unsupported-platform' ? 'unavailable' : 'unknown',
        status: gameIntegrationLabel(game), diagnostics: [setup?.message, setup?.liveMessage].filter((x): x is string => !!x), evidence: [`integration:${game.id}`, ...(activeGame === game.id ? ['active-bindings'] : [])], targets: [target('integration', game.id)],
        connections: [{ ...link(id, 'host', 'software', receiving, endpoints || undefined), observed: receiving || this.snapshot.components.find(previous => previous.id === id)?.connections.some(connection => connection.observed !== false && connection.endpoint === (endpoints || undefined)) || false }],
        forget: { allowed: false, reason: 'This integration is in the supported catalogue. Manage installation and settings in game details.', effect: '' } });
    }), diagnostics: state.error ? [state.error] : [] };
  }
  private async native(now: number): Promise<Capture> {
    const state = (await this.send({ command: 'system_inventory', offset: 0, limit: 32 })).systemInventory;
    if (!state || !Array.isArray(state.virtualDevices) || !Array.isArray(state.hiddenDevices)) throw Error('Native system inspection unavailable. Restart the updated service.');
    const hiddenDevices = [...state.hiddenDevices], associations = [...(state.controllerAssociations ?? [])]; let nextOffset = state.nextOffset;
    while (nextOffset != null) {
      if (!Number.isSafeInteger(nextOffset) || nextOffset !== hiddenDevices.length + associations.length || nextOffset > 4096) throw Error('Native inspection returned an invalid page.');
      const page = (await this.send({ command: 'system_inventory', offset: nextOffset, limit: 32, observedAt: state.observedAt })).systemInventory;
      const count = (page?.hiddenDevices?.length ?? 0) + (page?.controllerAssociations?.length ?? 0);
      if (!page || page.observedAt !== state.observedAt || page.total !== state.total || page.offset !== nextOffset || !count || nextOffset + count > page.total!) throw Error('Native system inventory changed during inspection. Refresh again.');
      hiddenDevices.push(...page.hiddenDevices); associations.push(...(page.controllerAssociations ?? [])); nextOffset = page.nextOffset;
    }
    return { provider: 'native', diagnostics: state.diagnostics, visibilityInspected: state.visibilityInspected === true || (state.visibilityInspected === undefined && !state.diagnostics.length), associations, observations: [
      ...state.virtualDevices.map(v => {
        const id = `virtual:${v.id}`, owned = v.owner ?? (v.ownerPid ? `PID ${v.ownerPid}` : null);
        return observation(id, v.name, 'virtual', 'native', now, { role: 'vJoy', presence: v.available === true ? 'connected' : v.available === false ? 'unavailable' : 'unknown', owner: owned, status: `${v.status}${v.createdByCockpit === true ? ' · Created by Cockpit' : ' · Creation provenance unknown'}`, diagnostics: v.diagnostics, evidence: [v.id], targets: [target('virtual', v.id)], connections: [link(id, 'host', 'software', v.available === true)], forget: { allowed: v.available === false && !owned, reason: 'Available or owned virtual devices cannot be forgotten.', effect: 'Forget only local virtual-device inventory metadata. The OS device, vJoy layout, driver and bindings are retained.' } });
      }),
      ...hiddenDevices.map(h => {
        const id = `visibility:${h.id}`;
        // Nearest bus evidence distinguishes direct USB from Bluetooth carried
        // through a USB radio. Parent chains are observations, never name guesses.
        const bus = [h.instanceId, ...(h.ancestors ?? [])].find(path => /^(USB|BTH|BTHLE|BTHENUM|BTHLEDEVICE)\\/i.test(path));
        const transport = bus && /^USB\\/i.test(bus) ? 'usb' as const : bus ? 'bluetooth' as const : 'unknown' as const;
        const ownControllers = h.controllerIds ?? (h.controllerId ? [h.controllerId] : []);
        const childControllers = /^USB\\VID_/i.test(h.instanceId) ? hiddenDevices.filter(child => (child.parentInstanceId ?? child.ancestors?.[0])?.toLowerCase() === h.instanceId.toLowerCase()).flatMap(child => child.controllerIds ?? (child.controllerId ? [child.controllerId] : [])) : [];
        return observation(id, h.name, 'controller', 'native', now, { role: 'Physical device', presence: h.present === true ? 'connected' : h.present === false ? 'offline' : 'unknown', hidden: h.hidden,
          status: h.configured ? 'Configured visibility rule' : '', diagnostics: [...h.diagnostics, ...(childControllers.length ? ['Visibility evidence includes the exact physical USB parent; application exceptions may limit its scope.'] : []), ...(h.inverse ? ['HidHide inverse application policy is active.'] : [])],
          evidence: [h.instanceId, ...[...new Set([...ownControllers, ...childControllers])].map(id => `controller:${id}`)], targets: [target('visibility', h.instanceId)],
          connections: [link(id, 'host', transport, h.present === true, h.interfacePaths?.[0] ?? h.instanceId)],
          forget: { allowed: h.present === false && h.configured === false && h.hidden === false, reason: h.configured || h.hidden ? 'Manage visibility rules explicitly. Inventory cleanup cannot remove HidHide configuration.' : 'Refresh device presence and HidHide visibility before cleanup.', effect: localEffect } });
      }),
    ] };
  }
  /** Inspect existing local bindings for confirmation only; never change or activate them. */
  async reviewForget(ids: string[]): Promise<SystemComponent[]> {
    const selected = this.snapshot.components.filter(c => ids.includes(c.id));
    const result = selected.map(c => ({ ...c, forget: { ...c.forget }, references: [...c.references] }));
    try {
      const profiles = (await this.send({ command: 'profile_list' })).profiles?.profiles;
      if (!profiles || profiles.length > 256) throw Error('Stored binding inventory unavailable.');
      for (const profile of profiles) {
        const assignments = (await this.send({ command: 'profile_assignments_get', id: profile.id })).profiles?.assignments;
        if (!assignments) throw Error(`Stored references for ${profile.name} unavailable.`);
        for (const c of result) {
          const impacted = this.snapshot.components.filter(related => related.id === c.id || (c.type === 'gateway' && related.connections.some(connection => connection.from === c.id)) || (c.type === 'companion' && related.connections.some(connection => connection.from === c.id)));
          const sourceIds = new Set(impacted.flatMap(component => [component.id, component.id.replace(/^resource:/, ''), ...component.targets.map(t => t.id), ...component.evidence.filter(e => e.startsWith('binding-source:')).map(e => e.slice(15))]));
          for (const a of assignments) if (sourceIds.has(a.sourceId)) c.references.push({ kind: 'binding', id: `${profile.id}/${a.resourceId}`, name: `${profile.name} / ${a.resourceId}` });
        }
      }
      for (const c of result) if (c.references.some(reference => reference.kind === 'binding')) {
        c.forget = { ...c.forget, allowed: false, reason: 'Used in profiles. Remove its profile assignments before cleaning up this device.' };
      }
    } catch (error) { for (const c of result) {
      c.diagnostics = [...c.diagnostics, `Affected stored references could not be fully inspected: ${message(error)}`];
      c.forget = { ...c.forget, allowed: false, reason: 'Profile usage could not be checked. Retry before cleaning up this device.' };
    } }
    return result;
  }
  /** Refresh and revalidate immediately before a one-shot guarded provider mutation. */
  async forget(id: string, signal?: AbortSignal): Promise<void> {
    signal?.throwIfAborted();
    await this.refresh();
    signal?.throwIfAborted();
    const current = this.snapshot.components.find(c => c.id === id);
    // A prior gateway removal or a concurrent refresh may already have retired
    // this row. There is no remaining provider or local record to remove.
    if (!current) return;
    if (!current.forget.allowed || current.presence === 'connected' || current.owner || current.hidden) throw Error(current.forget.reason || 'The selected component is no longer eligible.');
    const [c] = await this.reviewForget([id]);
    signal?.throwIfAborted();
    if (!c?.forget.allowed || c.presence === 'connected' || c.owner || c.hidden) throw Error(c?.forget.reason || 'The selected component is no longer eligible.');
    const t = c.targets.find(t => t.kind === 'controller' || t.kind === 'module' || t.kind === 'companion');
    this.epoch++;
    if (t?.kind === 'controller') {
      // The browser inventory outlives native capture IDs. Only forget a source
      // the fresh provider actually returned; absent IDs are local history.
      const captured = this.captures.get('controllers')?.observations.some(observation => observation.targets.some(target => target.kind === 'controller' && target.id === t.id));
      if (captured) await this.send({ command: 'controller_forget', sourceId: t.id });
    }
    else if (t?.kind === 'module' && this.gateways().some(e => e.id === t.gatewayId && e.state?.connected)) await this.send({ command: 'forget', gatewayId: t.gatewayId, moduleId: t.id });
    else if (t?.kind === 'companion') await this.send({ command: 'companion_revoke', companion: { companionId: t.id } });
    this.registry.forget(id); this.onChange(this.snapshot);
  }
  dispose() { this.disposed = true; this.epoch++; }
}
export const integrationGameId = (component: SystemComponent): GameId | undefined => component.targets.find(t => t.kind === 'integration')?.id as GameId | undefined;
