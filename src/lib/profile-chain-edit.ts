import { graphConnectionError, graphDraft, graphFreshId, type ProfileGraphDraft } from './profile-graph';
import { PROCESSING_PRESETS, processingNodeError, processingTemplate, type ProcessingPreset } from './profile-processing';
import type { DeepReadonly, ProfileDocument, ProfileProcessingNode } from './profiles';
import { signalOverview } from './profile-signal-overview';

const clone = <T>(value: DeepReadonly<T>): T => JSON.parse(JSON.stringify(value));
const same = (left: unknown, right: unknown): boolean => JSON.stringify(left) === JSON.stringify(right);

function chainNode(value: DeepReadonly<ProfileGraphDraft>, nodeId: string): DeepReadonly<ProfileProcessingNode> | undefined {
  return value.document.processingNodes.find(node => node.id === nodeId);
}

function chainBoundary(value: DeepReadonly<ProfileGraphDraft>, nodeId: string) {
  const node = chainNode(value, nodeId);
  if (!node || node.inputs.length < 1 || node.outputs.length !== 1) return null;
  const incoming = value.document.bindings.filter(binding => binding.to.nodeId === nodeId);
  const outgoing = value.document.bindings.filter(binding => binding.from.nodeId === nodeId);
  const inputIds = new Set(node.inputs.map(port => port.id));
  const connectedInputs = new Set(incoming.map(binding => binding.to.portId));
  if (incoming.length !== node.inputs.length || connectedInputs.size !== node.inputs.length || [...connectedInputs].some(id => !inputIds.has(id)) || outgoing.length !== 1) return null;
  return { node, incoming, outgoing };
}

function replacement(value: DeepReadonly<ProfileGraphDraft>, nodeId: string, preset: ProcessingPreset): ProfileProcessingNode | null {
  const boundary = chainBoundary(value, nodeId);
  if (!boundary) return null;
  const { node, incoming, outgoing } = boundary;
  try {
    let next = processingTemplate(preset, nodeId, node.inputs[0].contract, node.outputs[0].contract);
    if (['any', 'all', 'priority'].includes(preset)) {
      while (next.inputs.length < node.inputs.length) next = addChainInput(next);
    }
    if (next.inputs.length !== node.inputs.length || next.outputs.length !== 1) return null;
    // A replacement keeps the logical ports, and therefore every wire endpoint,
    // while requiring the same contracts at every input and output boundary.
    if (next.inputs.some((port, index) => !same(port.contract, node.inputs[index].contract)) || !same(next.outputs[0].contract, node.outputs[0].contract)) return null;
    for (const [index, input] of next.inputs.entries()) {
      input.id = node.inputs[index].id;
      input.name = node.inputs[index].name;
    }
    next.outputs[0].id = node.outputs[0].id;
    next.outputs[0].name = node.outputs[0].name;
    if (next.operation.kind === 'priority') {
      next.operation.order = node.operation.kind === 'priority'
        ? [...node.operation.order]
        : next.inputs.map(input => input.id);
    }
    next.name = isCustomName(node) ? node.name : next.name;
    const document = clone<ProfileDocument>(value.document);
    document.processingNodes = document.processingNodes.map(existing => existing.id === nodeId ? next : existing);
    if ([...incoming, ...outgoing].some(binding => graphConnectionError(document, binding.from, binding.to, binding.id))) return null;
    return next;
  } catch {
    return null;
  }
}

function defaultLabel(node: DeepReadonly<ProfileProcessingNode>): string | undefined {
  const kind = node.operation.kind;
  let preset: string = kind;
  if (kind === 'unit-convert' && node.operation.kind === 'unit-convert' && node.operation.from === 'ratio' && node.operation.to === '%') preset = 'ratio-to-percent';
  if (kind === 'builtin') preset = node.operation.rule.kind;
  return PROCESSING_PRESETS.find(([key]) => key === preset)?.[1];
}

function isCustomName(node: DeepReadonly<ProfileProcessingNode>): boolean {
  const label = defaultLabel(node);
  return !!label && node.name !== label;
}

/** Every preset can be selected; incompatible choices become provisional drafts in the editor. */
export function chainTransformOptions(value: DeepReadonly<ProfileGraphDraft>, nodeId: string): { value: ProcessingPreset; label: string }[] {
  if (!chainNode(value, nodeId)) return [];
  return PROCESSING_PRESETS.map(([preset, label]) => ({ value: preset, label }));
}

/** Append one typed input to a variadic Boolean OR, AND, or priority node. */
export function addChainInput(value: DeepReadonly<ProfileProcessingNode>): ProfileProcessingNode {
  const node = clone<ProfileProcessingNode>(value);
  if (!['any', 'all', 'priority'].includes(node.operation.kind)) {
    throw new Error('Add inputs is only supported for Boolean OR, Boolean AND, and Explicit input priority.');
  }
  if (node.inputs.length >= 8) throw new Error('This transformation already has the maximum of 8 inputs.');

  const used = new Set([...node.inputs, ...node.outputs].map(port => port.id));
  let index = node.inputs.length + 1;
  while (used.has(`input-${index}`)) index++;
  const id = `input-${index}`;
  node.inputs.push({ id, name: `Input ${index}`, direction: 'input', contract: clone(node.inputs[0].contract) });
  if (node.operation.kind === 'priority') node.operation.order.push(id);

  const error = processingNodeError(node);
  if (error) throw new Error(error);
  return node;
}

/** Replace an existing linear transform while preserving its node and port identities. */
export function replaceChainTransform(value: DeepReadonly<ProfileGraphDraft>, nodeId: string, preset: ProcessingPreset): ProfileGraphDraft {
  const boundary = chainBoundary(value, nodeId);
  if (!boundary) throw new Error('Replace a transform in a linear chain with every input and exactly one output route. Branched or incomplete transforms need their routes edited individually.');
  const nextNode = replacement(value, nodeId, preset);
  if (!nextNode) throw new Error('This transformation cannot preserve the existing input and output route contracts. Choose a compatible single-input/output transform.');
  const next = clone<ProfileGraphDraft>(value);
  next.document.processingNodes = next.document.processingNodes.map(node => node.id === nodeId ? nextNode : node);
  return next;
}

/** Copy shared stages on one terminal row before an edit changes their settings. */
export function instanceRowTransforms(value: DeepReadonly<ProfileGraphDraft>, laneId: string, activeVehicleTabId = ''): { draft: ProfileGraphDraft; nodeIds: Record<string, string>; bindingIds: Record<string, string> } {
  const base = graphDraft(value);
  const lane = [...new Map(signalOverview(base.document).flatMap(bundle => bundle.lanes).map(item => [item.id, item])).values()].find(item => item.id === laneId);
  if (!lane) throw new Error('Choose a current signal row before editing its shared transforms.');

  const laneBindings = new Set(lane.bindingIds);
  const shared = new Set(lane.transforms.filter(id => base.document.bindings.some(binding => binding.from.nodeId === id && !laneBindings.has(binding.id))));
  // A private copy of a shared stage also needs private transform ancestors so
  // its inputs can be rewired without changing another row's path.
  for (let size = -1; size !== shared.size;) {
    size = shared.size;
    for (const binding of base.document.bindings) {
      if (laneBindings.has(binding.id) && shared.has(binding.to.nodeId) && lane.transforms.includes(binding.from.nodeId)) shared.add(binding.from.nodeId);
    }
  }
  if (!shared.size) return { draft: base, nodeIds: {}, bindingIds: {} };

  const next = graphDraft(base), nodeIds: Record<string, string> = {}, bindingIds: Record<string, string> = {};
  for (const oldId of lane.transforms) {
    if (!shared.has(oldId)) continue;
    const source = base.document.processingNodes.find(node => node.id === oldId)!;
    const copy = clone<ProfileProcessingNode>(source);
    copy.id = graphFreshId(next.document, 'transform');
    nodeIds[oldId] = copy.id;
    next.document.processingNodes.push(copy);
    const tabId = activeVehicleTabId || base.document.vehicleTabs?.find(tab => tab.nodeIds.includes(oldId))?.id || '';
    if (tabId) {
      const tab = next.document.vehicleTabs?.find(item => item.id === tabId);
      if (!tab) throw new Error('Choose an existing vehicle tab for this signal row.');
      tab.nodeIds.push(copy.id);
    }
    const position = base.document.editor.positions.find(item => item.nodeId === oldId);
    if (position) next.document.editor.positions.push({ ...position, nodeId: copy.id });
  }

  for (const binding of base.document.bindings) {
    if (!laneBindings.has(binding.id)) continue;
    const fromId = nodeIds[binding.from.nodeId], toId = nodeIds[binding.to.nodeId];
    if (!fromId && !toId) continue;
    const from = fromId ? { ...binding.from, nodeId: fromId } : binding.from;
    const to = toId ? { ...binding.to, nodeId: toId } : binding.to;
    if (toId) {
      const id = graphFreshId(next.document, 'route');
      bindingIds[binding.id] = id;
      next.document.bindings.push({ id, from: clone(from), to: clone(to) });
    } else {
      const existing = next.document.bindings.find(item => item.id === binding.id)!;
      existing.from = clone(from);
      existing.to = clone(to);
    }
  }

  for (const binding of next.document.bindings) {
    const error = graphConnectionError(next.document, binding.from, binding.to, binding.id);
    if (error) throw new Error(`${binding.id}: ${error}`);
  }
  return { draft: graphDraft(next), nodeIds, bindingIds };
}
