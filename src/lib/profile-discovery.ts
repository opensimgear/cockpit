import { graphConnect, graphDraft, graphPort, graphPorts, graphSelectProviderPort, type ProfileGraphDraft } from './profile-graph';
import { RESOURCE_PORT_PAGE_SIZE, type ObservedResourcePort, type ProfileInventoryClient, type ResourceDetails } from './profile-inventory';
import { graphSelectCompanionPort, type ProfileCompanionAuthoringClient, type ProfileCompanionPage } from './profile-companion-authoring';
import type { DeepReadonly, ProfileEndpoint, ProfilePort, ProfileLocalAssignment } from './profiles';

/** Discovered choices remain local until a route uses their exact contract. */
export type ProfileDiscoveredPort = Omit<DeepReadonly<ProfilePort>, 'id'> & {
  id: string;
  key: string;
  profilePortId: string | null;
  sourceId: string | null;
  observed: DeepReadonly<ObservedResourcePort> | null;
  current: boolean;
  companionPage?: DeepReadonly<ProfileCompanionPage>;
};
export type ProfileDiscoveredEndpoint = { nodeId: string; port: DeepReadonly<ProfileDiscoveredPort> };
export type ProfilePreparedConnection = { draft: ProfileGraphDraft; from: ProfileEndpoint; to: ProfileEndpoint };

/** Physical HID identity stays exact; selectors display the control and its declared range. */
export function profilePhysicalPortPresentation(port: Pick<ProfileDiscoveredPort, 'id' | 'name' | 'observed' | 'contract'>): { label: string; description: string } | null {
  const identity = port.observed?.descriptor.id ?? (/^hid:/i.test(port.name) ? port.name : port.id);
  const hid = /^hid:([0-9a-f]{4}):([0-9a-f]{4}):\d+:\d+:\d+$/i.exec(identity);
  if (!hid || (port.contract.delivery !== 'state' && port.contract.delivery !== 'event')) return null;
  const page = parseInt(hid[1], 16), usage = parseInt(hid[2], 16), schema = port.contract.schema;
  const axes: Record<number, string> = { 0x30: 'X axis', 0x31: 'Y axis', 0x32: 'Z axis', 0x33: 'X rotation', 0x34: 'Y rotation', 0x35: 'Z rotation', 0x36: 'Slider', 0x37: 'Dial', 0x38: 'Wheel', 0x39: 'Hat switch' };
  const simulation: Record<number, string> = { 0xba: 'Rudder', 0xbb: 'Throttle', 0xc4: 'Accelerator', 0xc5: 'Brake', 0xc6: 'Clutch', 0xc8: 'Steering' };
  const friendly = page === 9 ? `Button ${usage}` : page === 1 ? axes[usage] : page === 2 ? simulation[usage] : undefined;
  if (!friendly) return null;
  const label = port.name && !/^hid:/i.test(port.name) ? port.name : friendly;
  if (schema.type === 'boolean') return { label, description: 'Boolean · 0–1' };
  if (schema.type === 'integer' || schema.type === 'number') return { label, description: `${schema.type === 'integer' ? 'Integer' : 'Number'} · ${schema.min}–${schema.max}${schema.unit ? ` ${schema.unit}` : ''}` };
  return null;
}
type DiscoveryDetails = DeepReadonly<ResourceDetails | ProfileCompanionPage> | null | undefined;

const variantKey = (port: DeepReadonly<ObservedResourcePort>) => JSON.stringify([port.descriptor.id, port.contractHash]);
const canonical = (value: unknown): string => JSON.stringify(value, (_key, item) => item && typeof item === 'object' && !Array.isArray(item) ? Object.fromEntries(Object.entries(item).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) : item);
const clone = <T>(value: DeepReadonly<T>): T => JSON.parse(JSON.stringify(value));
const freeze = <T>(value: T): DeepReadonly<T> => {
  if (value && typeof value === 'object' && !Object.isFrozen(value)) { Object.values(value).forEach(freeze); Object.freeze(value); }
  return value as DeepReadonly<T>;
};

/** Catalogues can exceed the graph's port limit; discover every page without declaring it. */
export async function profileDiscoverResource(inventory: Pick<ProfileInventoryClient, 'inspect'>, sourceId: string): Promise<DeepReadonly<ResourceDetails>> {
  const first = await inventory.inspect(sourceId, 0, RESOURCE_PORT_PAGE_SIZE);
  const ports: DeepReadonly<ObservedResourcePort>[] = [];
  let page = first;
  for (;;) {
    if (page.resource.id !== sourceId || page.offset !== ports.length || !Number.isSafeInteger(page.total) || page.total < 0 || page.total > 262144 || ports.length + page.ports.length > page.total || ports.length < page.total && !page.ports.length) throw new Error('Malformed resource detail page. Refresh its inputs and outputs.');
    if (page.session !== first.session || page.revision !== first.revision || page.total !== first.total || page.resource.generation !== first.resource.generation || page.resource.capabilityRevision !== first.resource.capabilityRevision) throw new Error('Resource capabilities or route changed while discovering inputs and outputs. Refresh the component.');
    ports.push(...page.ports);
    if (ports.length === first.total) break;
    page = await inventory.inspect(sourceId, ports.length, RESOURCE_PORT_PAGE_SIZE);
  }
  if (new Set(ports.map(variantKey)).size !== ports.length) throw new Error('Resource details contain duplicate port variants.');
  return freeze(clone<ResourceDetails>({ ...first, ports }));
}

/** Inspect the profile's captured publication even when the paired device runs another panel. */
export async function profileDiscoverCompanion(client: Pick<ProfileCompanionAuthoringClient, 'inspect'>, sourceId: string, publicationHash: string): Promise<DeepReadonly<ProfileCompanionPage>> {
  const first = await client.inspect(sourceId, publicationHash, 0, RESOURCE_PORT_PAGE_SIZE);
  const ports: DeepReadonly<ObservedResourcePort>[] = [];
  let page = first;
  for (;;) {
    if (page.schemaVersion !== 1 || page.resourceId !== sourceId || page.publicationHash !== publicationHash || page.offset !== ports.length || !Number.isSafeInteger(page.total) || page.total < 0 || page.total > 1024 || ports.length + page.ports.length > page.total || ports.length < page.total && !page.ports.length) throw new Error('Malformed companion port page. Refresh its captured panel.');
    if (page.identityFingerprint !== first.identityFingerprint || page.capabilityFingerprint !== first.capabilityFingerprint || page.total !== first.total) throw new Error('This paired companion or captured panel catalogue changed while discovering ports. Refresh the component.');
    if (page.ports.some(port => port.current || port.routeIds.length)) throw new Error('Prospective companion ports cannot claim live delivery authority.');
    ports.push(...page.ports);
    if (ports.length === first.total) break;
    page = await client.inspect(sourceId, publicationHash, ports.length, RESOURCE_PORT_PAGE_SIZE);
  }
  if (new Set(ports.map(port => port.descriptor.id)).size !== ports.length) throw new Error('Companion details contain duplicate port identities.');
  return freeze(clone<ProfileCompanionPage>({ ...first, ports }));
}

/** Saved IDs remain stable; companion choices come only from their captured publication. */
export function profileDiscoveredPorts(value: DeepReadonly<ProfileGraphDraft>, nodeId: string, details?: DeepReadonly<ResourceDetails> | DeepReadonly<ProfileCompanionPage> | null): ProfileDiscoveredPort[] {
  const assignment = value.assignments.find(row => row.resourceId === nodeId);
  const resource = value.document.resources.find(row => row.id === nodeId);
  const captured = value.document.companionAssignments.find(row => row.resourceId === nodeId);
  const companionPage = resource?.kind === 'companion' && assignment && details && 'resourceId' in details && details.resourceId === assignment.sourceId && details.publicationHash === captured?.publicationHash ? details : undefined;
  const catalogue = companionPage?.ports ?? (resource?.kind !== 'companion' && assignment && details && 'resource' in details && details.resource.id === assignment.sourceId ? details.ports : []);
  const byVariant = new Map(catalogue.map(port => [variantKey(port), port]));
  const pins = new Map(assignment?.ports.map(pin => [pin.profilePortId, pin]) ?? []);
  const selected = new Set<string>();
  const choices: ProfileDiscoveredPort[] = graphPorts(value.document, nodeId).map(port => {
    const pin = pins.get(port.id);
    const key = pin && JSON.stringify([pin.observedPortId, pin.expectedContractHash]);
    if (key) selected.add(key);
    const observed = key ? byVariant.get(key) ?? null : null;
    return { ...port, id: port.id, key: port.id, profilePortId: port.id, sourceId: assignment?.sourceId ?? null, observed, current: !resource || resource.kind === 'debug-controller' || observed?.current === true, ...(companionPage && observed ? { companionPage } : {}) };
  });
  for (const [variant, observed] of byVariant) {
    if ((!observed.current && !companionPage) || selected.has(variant)) continue;
    const key = `@observed:${variant}`;
    choices.push({ ...observed.descriptor, id: key, key, profilePortId: null, sourceId: assignment!.sourceId, observed, current: observed.current, ...(companionPage ? { companionPage } : {}) });
    selected.add(variant);
  }
  return choices;
}

/** Editor snapshots also change for pan, layout and unrelated routes. Keep
 * catalogue choices stable until this node's authored ports or discovery change. */
export class ProfileDiscoveredPortCache {
  private entries = new Map<string, {
    kind: string | undefined;
    ports: readonly DeepReadonly<ProfilePort>[];
    assignment: DeepReadonly<ProfileLocalAssignment> | undefined;
    publication: string | undefined;
    details: Exclude<DiscoveryDetails, undefined>;
    signature: string;
    choices: ProfileDiscoveredPort[];
  }>();
  private snapshot: Record<string, ProfileDiscoveredPort[]> = {};

  read(value: DeepReadonly<ProfileGraphDraft>, nodeId: string, details: DiscoveryDetails = null): ProfileDiscoveredPort[] {
    const kind = value.document.resources.find(resource => resource.id === nodeId)?.kind;
    const ports = graphPorts(value.document, nodeId);
    const assignment = value.assignments.find(row => row.resourceId === nodeId);
    const publication = kind === 'companion' ? value.document.companionAssignments.find(row => row.resourceId === nodeId)?.publicationHash : undefined;
    const catalogue = details ?? null;
    const previous = this.entries.get(nodeId);
    if (previous && previous.kind === kind && previous.ports === ports && previous.assignment === assignment && previous.publication === publication && previous.details === catalogue) return previous.choices;
    // History/layout edits clone the document, so reference identity alone is
    // insufficient. Only fingerprint authored data, never the full catalogue.
    const signature = JSON.stringify([kind ?? null, ports, assignment ?? null, publication ?? null]);
    const choices = previous?.signature === signature && previous.details === catalogue ? previous.choices : profileDiscoveredPorts(value, nodeId, catalogue);
    this.entries.set(nodeId, { kind, ports, assignment, publication, details: catalogue, signature, choices });
    return choices;
  }

  collect(value: DeepReadonly<ProfileGraphDraft>, requests: readonly { nodeId: string; details?: DiscoveryDetails }[]): Record<string, ProfileDiscoveredPort[]> {
    const next = Object.fromEntries(requests.map(request => [request.nodeId, this.read(value, request.nodeId, request.details)]));
    const wanted = new Set(requests.map(request => request.nodeId));
    for (const nodeId of this.entries.keys()) if (!wanted.has(nodeId)) this.entries.delete(nodeId);
    if (Object.keys(this.snapshot).length !== requests.length || requests.some(request => this.snapshot[request.nodeId] !== next[request.nodeId])) this.snapshot = next;
    return this.snapshot;
  }
}

function prepareEndpoint(value: ProfileGraphDraft, endpoint: DeepReadonly<ProfileDiscoveredEndpoint>): { draft: ProfileGraphDraft; endpoint: ProfileEndpoint } {
  const { nodeId, port } = endpoint;
  const assignment = value.assignments.find(row => row.resourceId === nodeId);
  const companion = value.document.resources.find(row => row.id === nodeId)?.kind === 'companion';
  if (port.sourceId !== (assignment?.sourceId ?? null)) throw new Error('This component assignment changed. Choose its input or output again.');
  if (companion && port.observed && !port.companionPage || port.companionPage && !companion) throw new Error('Choose a port from this companion’s exact captured panel.');
  if (port.profilePortId) {
    const saved = graphPort(value.document, { nodeId, portId: port.profilePortId });
    if (!saved || saved.direction !== port.direction || canonical(saved.contract) !== canonical(port.contract)) throw new Error('This input or output changed. Choose the port again.');
    if (port.observed && (!assignment?.ports.some(pin => pin.profilePortId === saved.id && pin.observedPortId === port.observed!.descriptor.id && pin.expectedContractHash === port.observed!.contractHash))) throw new Error('This port assignment changed. Choose its input or output again.');
    const draft = port.companionPage && port.observed ? graphSelectCompanionPort(value, nodeId, port.companionPage, port.observed) : value;
    return { draft, endpoint: { nodeId, portId: saved.id } };
  }
  if (!port.observed || !port.sourceId || (!companion && (!port.current || !port.observed.current)) || companion && !port.companionPage) throw new Error('Choose an advertised input or output.');
  const draft = companion ? graphSelectCompanionPort(value, nodeId, port.companionPage!, port.observed) : graphSelectProviderPort(value, nodeId, port.sourceId, port.observed);
  const pin = draft.assignments.find(row => row.resourceId === nodeId)!.ports.find(row => row.observedPortId === port.observed!.descriptor.id && row.expectedContractHash === port.observed!.contractHash)!;
  return { draft, endpoint: { nodeId, portId: pin.profilePortId } };
}

/** Preparation is atomic and also supports an explicit conversion before connecting. */
export function profilePrepareDiscoveredPorts(value: DeepReadonly<ProfileGraphDraft>, from: DeepReadonly<ProfileDiscoveredEndpoint>, to: DeepReadonly<ProfileDiscoveredEndpoint>): ProfilePreparedConnection {
  const source = prepareEndpoint(graphDraft(value), from);
  const target = prepareEndpoint(source.draft, to);
  return { draft: target.draft, from: source.endpoint, to: target.endpoint };
}

export function profileConnectDiscoveredPorts(value: DeepReadonly<ProfileGraphDraft>, from: DeepReadonly<ProfileDiscoveredEndpoint>, to: DeepReadonly<ProfileDiscoveredEndpoint>, replacingId?: string): ProfileGraphDraft {
  const prepared = profilePrepareDiscoveredPorts(value, from, to);
  return graphConnect(prepared.draft, prepared.from, prepared.to, replacingId);
}
