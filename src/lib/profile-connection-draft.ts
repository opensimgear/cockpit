import { canvasConnectionPlan } from './profile-canvas-construction';
import { graphConnect, graphDisconnect, graphFreshId, graphRemoveNode, graphRemovePort, graphSetProcessing, type ProfileGraphDraft } from './profile-graph';
import type { ProfileDiscoveredEndpoint } from './profile-discovery';
import { processingNodeError, processingTemplate, type ProcessingPreset } from './profile-processing';
import { graphContractError } from './profile-graph';
import type { DeepReadonly, ProfileBinding, ProfilePortContract, ProfileProcessingNode } from './profiles';

const clone = <T>(value: DeepReadonly<T>): T => JSON.parse(JSON.stringify(value));
const message = (error: unknown): string => error instanceof Error ? error.message : String(error);

type ConnectionPlan = {
  draft: ProfileGraphDraft;
  nodes: ProfileProcessingNode[];
  writer?: DeepReadonly<ProfileBinding>;
  error: string;
};

function failed(value: DeepReadonly<ProfileGraphDraft>, error: string): ConnectionPlan {
  return { draft: clone<ProfileGraphDraft>(value), nodes: [], error };
}

function commitPrepared(plan: ReturnType<typeof canvasConnectionPlan>, replacingId?: string, replaceOccupied = false): ProfileGraphDraft {
  if (plan.error) throw new Error(plan.error);
  if (plan.writer && !replaceOccupied) throw new Error('This input is already connected. Choose Replace connection to change its writer.');
  let next = plan.draft;
  if (plan.writer && plan.writer.id !== replacingId) {
    next = graphDisconnect(next, plan.writer.id);
    if (replacingId) return graphConnect(next, plan.from, plan.to, replacingId);
    const connected = graphConnect(next, plan.from, plan.to);
    const created = connected.document.bindings.find(binding => binding.from.nodeId === plan.from.nodeId && binding.from.portId === plan.from.portId && binding.to.nodeId === plan.to.nodeId && binding.to.portId === plan.to.portId)!;
    connected.document.bindings = connected.document.bindings.map(binding => binding.id === created.id ? { ...binding, id: plan.writer!.id } : binding);
    return connected;
  }
  return graphConnect(next, plan.from, plan.to, replacingId ?? plan.writer?.id);
}

function pruneReplacedSources(
  value: ProfileGraphDraft,
  formerBindings: readonly DeepReadonly<ProfileBinding>[],
): ProfileGraphDraft {
  const pending = formerBindings.map(binding => binding.from);
  let next = value;
  while (pending.length) {
    const endpoint = pending.pop()!;
    if (next.document.bindings.some(binding => binding.from.nodeId === endpoint.nodeId && binding.from.portId === endpoint.portId)) continue;
    const processing = next.document.processingNodes.find(node => node.id === endpoint.nodeId);
    if (processing) {
      if (next.document.bindings.some(binding => binding.from.nodeId === processing.id)) continue;
      const incoming = next.document.bindings.filter(binding => binding.to.nodeId === processing.id).map(binding => binding.from);
      next = graphRemoveNode(next, processing.id);
      pending.push(...incoming);
      continue;
    }
    const resource = next.document.resources.find(node => node.id === endpoint.nodeId);
    if (resource?.ports.some(port => port.id === endpoint.portId)) next = graphRemovePort(next, resource.id, endpoint.portId);
  }
  return next;
}

function transformInput(node: DeepReadonly<ProfileProcessingNode>, source: DeepReadonly<ProfilePortContract>) {
  const compatible = node.inputs.filter(port => !graphContractError(source, port.contract));
  return compatible.find(port => port.id === 'value') ?? compatible[0];
}

/** Build an isolated preview. Provider pins, transforms and edges stay in this returned candidate until commit. */
export function connectionDraftPlan(
  value: DeepReadonly<ProfileGraphDraft>,
  from: ProfileDiscoveredEndpoint,
  to: ProfileDiscoveredEndpoint,
  transforms: readonly DeepReadonly<ProfileProcessingNode>[],
  replacingId?: string,
  activeVehicleTabId = '',
): ConnectionPlan {
  try {
    if (transforms.length > 32) throw new Error('A connection can contain at most 32 transforms.');
    if (replacingId && !value.document.bindings.some(binding => binding.id === replacingId)) throw new Error('This connection was removed. Choose an existing wire to edit.');
    let working = clone<ProfileGraphDraft>(value);
    let tail = from;
    const nodes: ProfileProcessingNode[] = [];
    for (const item of transforms) {
      const node = clone<ProfileProcessingNode>(item);
      if (node.draft) throw new Error('Configure each transformation before adding it to a connection.');
      const issue = processingNodeError(node);
      if (issue) throw new Error(issue);
      if (node.outputs.length !== 1) throw new Error('A connection transformation needs one declared output.');
      if (working.document.processingNodes.some(existing => existing.id === node.id) || nodes.some(existing => existing.id === node.id)) throw new Error('Choose a unique transformation identity.');
      const input = transformInput(node, tail.port.contract);
      if (!input) throw new Error('This transformation has no input compatible with the current signal.');
      working = graphSetProcessing(working, node, activeVehicleTabId);
      const endpoint = (port: DeepReadonly<ProfileProcessingNode['inputs'][number]>): ProfileDiscoveredEndpoint => ({
        nodeId: node.id,
        port: { ...port, key: port.id, profilePortId: port.id, sourceId: null, observed: null, current: true },
      });
      const inputEndpoint = endpoint(input);
      const incoming = canvasConnectionPlan(working, tail, inputEndpoint);
      working = commitPrepared(incoming);
      const output = node.outputs[0];
      if (!output) throw new Error('A connection transformation needs one declared output.');
      tail = endpoint(output as ProfileProcessingNode['inputs'][number]);
      nodes.push(node);
    }
    const final = canvasConnectionPlan(working, tail, to, replacingId);
    if (final.error) throw new Error(final.error);
    const complete = commitPrepared(final, replacingId, true);
    return { ...final, draft: complete, nodes, error: '' };
  } catch (error) {
    return failed(value, message(error));
  }
}

/** Commit the entire preview as one returned graph value; occupied writers need explicit confirmation. */
export function commitConnectionDraft(
  value: DeepReadonly<ProfileGraphDraft>,
  from: ProfileDiscoveredEndpoint,
  to: ProfileDiscoveredEndpoint,
  transforms: readonly DeepReadonly<ProfileProcessingNode>[],
  replacingId?: string,
  replaceOccupied = false,
  activeVehicleTabId = '',
): ProfileGraphDraft {
  const plan = connectionDraftPlan(value, from, to, transforms, replacingId, activeVehicleTabId);
  if (plan.error) throw new Error(plan.error);
  if (plan.writer && !replaceOccupied) throw new Error('This input is already connected. Choose Replace connection to change its writer.');
  const formerBindings = [
    ...(replacingId ? value.document.bindings.filter(binding => binding.id === replacingId) : []),
    ...(plan.writer && plan.writer.id !== replacingId ? [plan.writer] : []),
  ];
  return pruneReplacedSources(plan.draft, formerBindings);
}

/** Discard one committed terminal row and prune only its now-unused upstream graph. */
export function discardCommittedConnection(value: DeepReadonly<ProfileGraphDraft>, terminalBindingId: string): ProfileGraphDraft {
  const terminal = value.document.bindings.find(binding => binding.id === terminalBindingId);
  if (!terminal) throw new Error('Choose an existing terminal route to discard.');
  if (value.document.processingNodes.some(node => node.id === terminal.to.nodeId)) throw new Error('Choose the terminal route into a resource or output, not an internal transformation route.');
  let next = pruneReplacedSources(graphDisconnect(value, terminalBindingId), [terminal]);
  const target = next.document.resources.find(resource => resource.id === terminal.to.nodeId);
  const pinned = next.assignments.some(assignment => assignment.resourceId === terminal.to.nodeId && assignment.ports.some(pin => pin.profilePortId === terminal.to.portId));
  const referenced = next.document.bindings.some(binding =>
    binding.from.nodeId === terminal.to.nodeId && binding.from.portId === terminal.to.portId ||
    binding.to.nodeId === terminal.to.nodeId && binding.to.portId === terminal.to.portId,
  );
  if (target?.ports.some(port => port.id === terminal.to.portId) && pinned && !referenced) next = graphRemovePort(next, target.id, terminal.to.portId);
  return next;
}

/** Make one compatible, single-output stage with an identity not used by the graph or open draft. */
export function createConnectionTransform(
  value: DeepReadonly<ProfileGraphDraft>,
  inputContract: DeepReadonly<ProfilePortContract>,
  targetContract: DeepReadonly<ProfilePortContract>,
  preset: ProcessingPreset,
  existingDrafts: readonly DeepReadonly<ProfileProcessingNode>[] = [],
): ProfileProcessingNode {
  const used = new Set([
    ...value.document.resources.map(node => node.id),
    ...value.document.resources.flatMap(node => node.ports.map(port => port.id)),
    ...value.document.processingNodes.map(node => node.id),
    ...value.document.processingNodes.flatMap(node => [...node.inputs, ...node.outputs].map(port => port.id)),
    ...value.document.virtualOutputs.map(node => node.id),
    ...value.document.virtualOutputs.flatMap(node => node.ports.map(port => port.id)),
    ...(value.document.desktopOutputs ?? []).map(node => node.id),
    ...(value.document.desktopOutputs ?? []).flatMap(node => node.ports.map(port => port.id)),
    ...value.document.bindings.map(binding => binding.id),
    ...existingDrafts.map(node => node.id),
    ...existingDrafts.flatMap(node => [...node.inputs, ...node.outputs].map(port => port.id)),
  ]);
  let id = graphFreshId(value.document, 'transform');
  for (let suffix = 1; used.has(id); suffix++) id = `transform-${suffix + 1}`;
  const node = processingTemplate(preset, id, inputContract, targetContract);
  if (!transformInput(node, inputContract) || node.outputs.length !== 1) throw new Error('Choose a transformation with a compatible input and one output.');
  return node;
}
