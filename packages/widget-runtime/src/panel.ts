import {
  ARTIFACT_LIMITS, BROWSER_WIDGET_CAPABILITIES, TILE_BYTES, canonicalWidget,
  validateCompiledArtifact, verifyCompiledBundle, widgetDigest,
  type CompiledWidgetArtifact, type CompiledWidgetBundle, type CompiledAssetDescriptor,
  type CompiledAssetPayload, type CompiledPort, type WidgetCapabilities,
} from './artifact.ts';
import type { BehaviorDefinition, BehaviorFeedback, Scalar } from './behavior.ts';

export interface PanelBounds { x: number; y: number; width: number; height: number }
export type PanelControl = { id: string; label: string } & (
  { kind: 'button' } | { kind: 'axis'; axisMode: 'unipolar'; fallback: number }
);
export interface PanelFeedback { id: string; label: string; channelId: string; kind: 'indicator' | 'gauge' | 'text' }
export interface PanelWidget {
  id: string; pageId: string; label: string; bounds: PanelBounds; artifactHash: string;
  inputBindings: { portId: string; controlId: string }[];
  outputBindings: { portId: string; feedbackId: string }[];
}
export interface CompiledPanelDocument {
  schemaVersion: 4;
  ui: {
    schemaVersion: 2; id: string; title: string; controls: PanelControl[];
    pages: { id: string; title: string }[]; feedbackWidgets: PanelFeedback[];
    assets: CompiledAssetDescriptor[];
  };
  presentation: {
    format: 'compiled-raster-v1';
    viewports: { pageId: string; width: number; height: number; background?: string }[];
    widgets: PanelWidget[];
    artifacts: { hash: string; artifact: CompiledWidgetArtifact }[];
  };
  targets: { id: string; label: string; axes: 'unipolar'[]; buttons: number; hats: 0 }[];
  bindings: { controlId: string; targetId: string; kind: 'axis' | 'button'; index: number }[];
}
export interface CompiledPanelPublication {
  document: CompiledPanelDocument; assets: CompiledAssetPayload[];
  publicationHash: string; capabilityFingerprint: string;
}
export interface PanelPlacement {
  id: string; pageId: string; label: string; bounds: PanelBounds; bundle: CompiledWidgetBundle;
}
export interface PanelResourceReport {
  assetBytes: number; manifestBytes: number; residentBytes: number; activeCandidateBytes: number;
}
export type PanelFeedbackSamples = Record<string, BehaviorFeedback>;
type Validation<T> = { ok: true; value: T } | { ok: false; error: string };
const encoder = new TextEncoder();
function ensure(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function object(value: unknown, required: string[], optional: string[] = []): Record<string, any> {
  ensure(value && typeof value === 'object' && !Array.isArray(value) && Object.getPrototypeOf(value) === Object.prototype, 'Expected a plain panel object');
  const item = value as Record<string, any>;
  ensure(required.every(key => Object.hasOwn(item, key)) && Object.keys(item).every(key => required.includes(key) || optional.includes(key)), 'Unexpected or missing panel field');
  return item;
}
function list(value: unknown, min: number, max: number): any[] { ensure(Array.isArray(value) && value.length >= min && value.length <= max, 'Panel list outside bounds'); return value; }
function stable(value: unknown): asserts value is string { ensure(typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value), 'Invalid panel ID'); }
function text(value: unknown): asserts value is string { ensure(typeof value === 'string' && value.trim() && value.length <= 120 && !/[\u0000-\u001f]/.test(value), 'Invalid panel text'); }
function integer(value: unknown, min: number, max: number): void { ensure(Number.isSafeInteger(value) && Number(value) >= min && Number(value) <= max, 'Panel integer outside bounds'); }
function digest(value: unknown): void { ensure(typeof value === 'string' && /^[a-f0-9]{64}$/.test(value), 'Invalid panel artifact hash'); }
function sorted<T extends Record<string, any>>(values: T[], key: keyof T): T[] { return [...values].sort((a, b) => String(a[key]) < String(b[key]) ? -1 : String(a[key]) > String(b[key]) ? 1 : 0); }
function portSemantics(port: CompiledPort) { const { label: _, ...semantics } = port; return semantics; }

/** Cues do not change the identity of control behavior. The resulting projection is
 * recomputed from admitted data on both host and player, never from a claimed hash. */
export function panelBehaviorSemantics(definition: BehaviorDefinition): BehaviorDefinition {
  return {
    ...definition,
    regions: definition.regions.map(region => ({ ...region, transitions: region.transitions.map(transition => ({
      ...transition, actions: (transition.actions ?? []).filter(action => action.type !== 'cue'),
    })) })),
  };
}
export function compiledPanelCapabilityProjection(document: CompiledPanelDocument): unknown {
  return {
    uiId: document.ui.id,
    controls: sorted(document.ui.controls.map(({ label: _, ...control }) => control), 'id'),
    feedback: sorted(document.ui.feedbackWidgets.map(({ label: _, ...feedback }) => feedback), 'id'),
    widgets: sorted(document.presentation.widgets.map(widget => {
      const artifact = document.presentation.artifacts.find(item => item.hash === widget.artifactHash)!.artifact;
      return { id: widget.id, behavior: panelBehaviorSemantics(artifact.behavior),
        inputs: sorted(artifact.inputs.map(portSemantics), 'id'), outputs: sorted(artifact.outputs.map(portSemantics), 'id'),
        inputBindings: sorted(widget.inputBindings, 'portId'), outputBindings: sorted(widget.outputBindings, 'portId') };
    }), 'id'),
    targets: sorted(document.targets.map(({ label: _, ...target }) => target), 'id'),
    bindings: sorted(document.bindings, 'controlId'),
  };
}
function accepts(port: CompiledPort, value: Scalar): boolean {
  return port.type === 'boolean' ? typeof value === 'boolean'
    : port.type === 'number' ? typeof value === 'number' && Number.isFinite(value) && value >= port.min! && value <= port.max!
    : typeof value === 'string' && port.values!.includes(value);
}
function normalizedPortValue(port: CompiledPort, value: Scalar): boolean | number {
  ensure(accepts(port, value), 'Value outside compiled input domain');
  return port.type === 'boolean' ? value as boolean : port.type === 'number'
    ? ((value as number) - port.min!) / (port.max! - port.min!)
    : port.values!.indexOf(value as string) / (port.values!.length - 1);
}
function inputNeutral(artifact: CompiledWidgetArtifact, port: CompiledPort): Scalar {
  const output = artifact.behavior.outputs?.find(output => output.id === port.id);
  ensure(output, 'Missing compiled input neutral');
  return output.neutral;
}
async function endpointId(widgetId: string, direction: 'in' | 'out', portId: string): Promise<string> {
  const readable = `${widgetId}_${direction}_${portId}`;
  return readable.length <= 64 ? readable : `${widgetId.slice(0, 16)}_${direction}_${(await widgetDigest(encoder.encode(canonicalWidget({ widgetId, direction, portId })))).slice(0, 32)}`;
}
export function compiledPanelResources(document: CompiledPanelDocument): PanelResourceReport {
  const manifestBytes = encoder.encode(canonicalWidget(document)).length;
  const assetBytes = document.ui.assets.reduce((sum, asset) => sum + asset.sizeBytes, 0);
  const residentBytes = assetBytes * 3 + manifestBytes * 4 + document.presentation.widgets.reduce((sum, widget) => {
    const a = document.presentation.artifacts.find(item => item.hash === widget.artifactHash)!.artifact;
    return sum + a.resources.dictionaryBytes + a.width * a.height * 8 + TILE_BYTES + a.resources.frameMapBytes * 2 + a.resources.audioWorkingBytes;
  }, 0);
  return { assetBytes, manifestBytes, residentBytes, activeCandidateBytes: residentBytes * 2 };
}
export function validateCompiledPanelDocument(input: unknown, capabilities: WidgetCapabilities = BROWSER_WIDGET_CAPABILITIES): Validation<CompiledPanelDocument> {
  try {
    const serialized = canonicalWidget(input);
    ensure(encoder.encode(serialized).length <= ARTIFACT_LIMITS.manifestBytes, 'Compiled panel manifest exceeds 1 MiB');
    const d = object(JSON.parse(serialized), ['schemaVersion', 'ui', 'presentation', 'targets', 'bindings']);
    ensure(d.schemaVersion === 4, 'Unsupported compiled panel version');
    const ui = object(d.ui, ['schemaVersion', 'id', 'title', 'controls', 'pages', 'feedbackWidgets', 'assets']);
    ensure(ui.schemaVersion === 2, 'Unsupported compiled logical UI'); stable(ui.id); text(ui.title);
    const ids = new Set<string>();
    const unique = (value: unknown) => { stable(value); ensure(!ids.has(value), 'Duplicate panel ID'); ids.add(value); };
    const controls = list(ui.controls, 0, 128);
    for (const control of controls) {
      object(control, control.kind === 'axis' ? ['id', 'label', 'kind', 'axisMode', 'fallback'] : ['id', 'label', 'kind']);
      unique(control.id); text(control.label);
      ensure(control.kind === 'button' || control.kind === 'axis' && control.axisMode === 'unipolar' && typeof control.fallback === 'number' && Number.isFinite(control.fallback) && control.fallback >= 0 && control.fallback <= 1, 'Unsupported compiled control endpoint');
    }
    const feedback = list(ui.feedbackWidgets, 0, 64), channels = new Set<string>();
    for (const f of feedback) { object(f, ['id', 'label', 'channelId', 'kind']); unique(f.id); text(f.label); stable(f.channelId); ensure(!channels.has(f.channelId), 'Duplicate feedback channel'); channels.add(f.channelId); ensure(['indicator', 'gauge', 'text'].includes(f.kind), 'Invalid feedback type'); }
    const pages = list(ui.pages, 1, 16);
    for (const page of pages) { object(page, ['id', 'title']); unique(page.id); text(page.title); }
    const assets = list(ui.assets, 1, ARTIFACT_LIMITS.assets);
    for (const asset of assets) { unique(asset.id); }
    const p = object(d.presentation, ['format', 'viewports', 'widgets', 'artifacts']);
    ensure(p.format === 'compiled-raster-v1', 'Unsupported compiled panel presentation');
    const viewports = list(p.viewports, pages.length, pages.length), viewPages = new Set<string>();
    for (const viewport of viewports) {
      object(viewport, ['pageId', 'width', 'height'], ['background']);
      if(Object.hasOwn(viewport,'background'))ensure(typeof viewport.background==='string'&&/^#[0-9a-fA-F]{6}$/.test(viewport.background),'Invalid panel background color'); ensure(pages.some(page => page.id === viewport.pageId) && !viewPages.has(viewport.pageId), 'Invalid viewport page'); viewPages.add(viewport.pageId);
      integer(viewport.width, 320, 8192); integer(viewport.height, 320, 8192);
    }
    const artifacts = list(p.artifacts, 1, 192), hashes = new Set<string>(), artifactAssets = new Map<string, CompiledAssetDescriptor>();
    for (const entry of artifacts) {
      object(entry, ['hash', 'artifact']); digest(entry.hash); ensure(!hashes.has(entry.hash), 'Duplicate compiled artifact'); hashes.add(entry.hash);
      const result = validateCompiledArtifact(entry.artifact, capabilities); ensure(result.ok, result.ok ? '' : result.error);
      for (const asset of entry.artifact.assets) { const old = artifactAssets.get(asset.id); ensure(!old || canonicalWidget(old) === canonicalWidget(asset), 'Conflicting compiled asset identity'); artifactAssets.set(asset.id, asset); }
    }
    ensure(assets.length === artifactAssets.size && assets.every(asset => canonicalWidget(asset) === canonicalWidget(artifactAssets.get(asset.id))), 'Compiled panel asset closure mismatch');
    const widgets = list(p.widgets, 1, 192), usedArtifacts = new Set<string>(), boundControls = new Set<string>(), boundFeedback = new Set<string>();
    for (const widget of widgets) {
      object(widget, ['id', 'pageId', 'label', 'bounds', 'artifactHash', 'inputBindings', 'outputBindings']); unique(widget.id); text(widget.label);
      const viewport = viewports.find(v => v.pageId === widget.pageId); ensure(viewport, 'Unknown widget page');
      const bounds = object(widget.bounds, ['x', 'y', 'width', 'height']);
      ensure(Object.values(bounds).every(v => typeof v === 'number' && Number.isFinite(v) && v >= 0 && v <= 1) && bounds.width > 0 && bounds.height > 0 && bounds.x + bounds.width <= 1 + 1e-10 && bounds.y + bounds.height <= 1 + 1e-10, 'Widget outside panel');
      const artifact = artifacts.find(entry => entry.hash === widget.artifactHash)?.artifact as CompiledWidgetArtifact | undefined;
      ensure(artifact && artifact.width === Math.round(bounds.width * viewport.width) && artifact.height === Math.round(bounds.height * viewport.height), 'Widget raster size differs from placement; recompile'); usedArtifacts.add(widget.artifactHash);
      const inputs = list(widget.inputBindings, artifact.inputs.length, artifact.inputs.length), ports = new Set<string>();
      for (const binding of inputs) {
        object(binding, ['portId', 'controlId']); const port = artifact.inputs.find(port => port.id === binding.portId), control = controls.find(control => control.id === binding.controlId);
        ensure(port && control && !ports.has(port.id) && !boundControls.has(control.id), 'Invalid or duplicate widget input binding'); ports.add(port.id); boundControls.add(control.id);
        ensure(port.type !== 'enum' || port.values!.length > 1, 'A controller enum needs at least two values');
        const neutral = normalizedPortValue(port, inputNeutral(artifact, port));
        ensure(port.type === 'boolean' ? control.kind === 'button' && neutral === false : control.kind === 'axis' && control.axisMode === 'unipolar' && control.fallback === neutral, 'Compiled input endpoint/neutral mismatch');
      }
      const outputs = list(widget.outputBindings, artifact.outputs.length, artifact.outputs.length), outputPorts = new Set<string>();
      for (const binding of outputs) {
        object(binding, ['portId', 'feedbackId']); const port = artifact.outputs.find(port => port.id === binding.portId), f = feedback.find(f => f.id === binding.feedbackId);
        ensure(port && f && !outputPorts.has(port.id) && !boundFeedback.has(f.id), 'Invalid or duplicate widget feedback binding'); outputPorts.add(port.id); boundFeedback.add(f.id);
        ensure(f.kind === (port.type === 'boolean' ? 'indicator' : port.type === 'number' ? 'gauge' : 'text'), 'Compiled feedback endpoint type mismatch');
        ensure(artifact.behavior.signals.some(signal => signal.id === port.signal && signal.source === 'feedback') && artifact.behavior.events.some(event => event.updates?.includes(port.signal)), 'Feedback port needs an event admitting its feedback signal');
      }
    }
    ensure(usedArtifacts.size === artifacts.length && boundControls.size === controls.length && boundFeedback.size === feedback.length, 'Unbound compiled panel data');
    ensure(pages.every(page => widgets.some(widget => widget.pageId === page.id)), 'Every page needs a compiled widget');
    const targets = list(d.targets, controls.length ? 1 : 0, 8);
    for (const target of targets) {
      object(target, ['id', 'label', 'axes', 'buttons', 'hats']); unique(target.id); text(target.label); ensure(list(target.axes, 0, 16).every(mode => mode === 'unipolar'), 'Invalid compiled target axes'); integer(target.buttons, 0, 128); ensure(target.hats === 0 && (target.axes.length || target.buttons), 'Invalid compiled target');
    }
    const bindings = list(d.bindings, controls.length, controls.length), used = new Set<string>(), slots = new Set<string>();
    for (const binding of bindings) {
      object(binding, ['controlId', 'targetId', 'kind', 'index']); const control = controls.find(control => control.id === binding.controlId), target = targets.find(target => target.id === binding.targetId);
      ensure(control && target && !used.has(control.id), 'Invalid logical binding'); integer(binding.index, 0, 127);
      const slot = `${target.id}:${binding.kind}:${binding.index}`; ensure(!slots.has(slot), 'Duplicate target slot'); slots.add(slot); used.add(control.id);
      ensure(binding.kind === control.kind && (binding.kind === 'button' ? binding.index < target.buttons : binding.kind === 'axis' && binding.index < target.axes.length), 'Incompatible logical binding');
    }
    const document = d as CompiledPanelDocument, resources = compiledPanelResources(document);
    ensure(resources.assetBytes <= capabilities.maxTotalBytes && resources.activeCandidateBytes <= capabilities.maxResidentBytes, 'Compiled panel exceeds target asset or active/candidate memory budget');
    return { ok: true, value: document };
  } catch (error) { return { ok: false, error: error instanceof Error ? error.message : 'Invalid compiled panel' }; }
}
export async function verifyCompiledPanelPublication(input: unknown, capabilities: WidgetCapabilities = BROWSER_WIDGET_CAPABILITIES): Promise<CompiledPanelPublication> {
  const publication = object(input, ['document', 'assets', 'publicationHash', 'capabilityFingerprint']);
  const result = validateCompiledPanelDocument(publication.document, capabilities); ensure(result.ok, result.ok ? '' : result.error); const document = result.value;
  ensure('sha256:' + await widgetDigest(encoder.encode(canonicalWidget(document))) === publication.publicationHash, 'Compiled panel content hash mismatch');
  ensure('sha256:' + await widgetDigest(encoder.encode(canonicalWidget(compiledPanelCapabilityProjection(document)))) === publication.capabilityFingerprint, 'Compiled panel capability fingerprint mismatch');
  const payloads = list(publication.assets, document.ui.assets.length, document.ui.assets.length), ids = new Set<string>();
  for (const payload of payloads) { object(payload, ['assetId', 'base64']); ensure(document.ui.assets.some(asset => asset.id === payload.assetId) && !ids.has(payload.assetId), 'Unknown or duplicate panel payload'); ids.add(payload.assetId); }
  for (const entry of document.presentation.artifacts) await verifyCompiledBundle({ artifact: entry.artifact, artifactHash: entry.hash, assets: payloads.filter(payload => entry.artifact.assets.some(asset => asset.id === payload.assetId)) }, capabilities);
  return { document, assets: structuredClone(payloads), publicationHash: publication.publicationHash, capabilityFingerprint: publication.capabilityFingerprint };
}
export async function createCompiledPanel(options: { id: string; title: string; viewports: { pageId: string; title: string; width: number; height: number; background?: string }[] }, placements: PanelPlacement[], capabilities: WidgetCapabilities = BROWSER_WIDGET_CAPABILITIES): Promise<CompiledPanelPublication> {
  const artifacts = new Map<string, CompiledWidgetArtifact>(), descriptors = new Map<string, CompiledAssetDescriptor>(), payloads = new Map<string, CompiledAssetPayload>();
  const controls: PanelControl[] = [], feedbackWidgets: PanelFeedback[] = [], widgets: PanelWidget[] = [], bindings: CompiledPanelDocument['bindings'] = [];
  let buttons = 0, axes = 0;
  const targetId = 'controller';
  for (const placement of placements) {
    const bundle = await verifyCompiledBundle(placement.bundle, capabilities), artifact = bundle.artifact;
    artifacts.set(bundle.artifactHash, artifact);
    for (const descriptor of artifact.assets) { const old = descriptors.get(descriptor.id); ensure(!old || canonicalWidget(old) === canonicalWidget(descriptor), 'Conflicting compiled asset'); descriptors.set(descriptor.id, descriptor); }
    for (const payload of bundle.assets) payloads.set(payload.assetId, payload);
    const inputBindings = [];
    for (const port of artifact.inputs) {
      const id = await endpointId(placement.id, 'in', port.id), neutral = normalizedPortValue(port, inputNeutral(artifact, port));
      if (port.type === 'boolean') { controls.push({ id, label: `${placement.label} ${port.label}`.slice(0, 120), kind: 'button' }); bindings.push({ controlId: id, targetId, kind: 'button', index: buttons++ }); }
      else { controls.push({ id, label: `${placement.label} ${port.label}`.slice(0, 120), kind: 'axis', axisMode: 'unipolar', fallback: neutral as number }); bindings.push({ controlId: id, targetId, kind: 'axis', index: axes++ }); }
      inputBindings.push({ portId: port.id, controlId: id });
    }
    const outputBindings = [];
    for (const port of artifact.outputs) {
      const id = await endpointId(placement.id, 'out', port.id);
      feedbackWidgets.push({ id, label: `${placement.label} ${port.label}`.slice(0, 120), channelId: id, kind: port.type === 'boolean' ? 'indicator' : port.type === 'number' ? 'gauge' : 'text' });
      outputBindings.push({ portId: port.id, feedbackId: id });
    }
    widgets.push({ id: placement.id, pageId: placement.pageId, label: placement.label, bounds: placement.bounds, artifactHash: bundle.artifactHash, inputBindings, outputBindings });
  }
  const document: CompiledPanelDocument = {
    schemaVersion: 4,
    ui: { schemaVersion: 2, id: options.id, title: options.title, controls, pages: options.viewports.map(v => ({ id: v.pageId, title: v.title })), feedbackWidgets, assets: sorted([...descriptors.values()], 'id') },
    presentation: { format: 'compiled-raster-v1', viewports: options.viewports.map(({ title: _, ...viewport }) => viewport), widgets, artifacts: sorted([...artifacts].map(([hash, artifact]) => ({ hash, artifact })), 'hash') },
    targets: controls.length ? [{ id: targetId, label: options.title, axes: Array.from({ length: axes }, () => 'unipolar' as const), buttons, hats: 0 }] : [], bindings,
  };
  const publication = { document, assets: sorted([...payloads.values()], 'assetId'), publicationHash: 'sha256:' + await widgetDigest(encoder.encode(canonicalWidget(document))), capabilityFingerprint: 'sha256:' + await widgetDigest(encoder.encode(canonicalWidget(compiledPanelCapabilityProjection(document)))) };
  return verifyCompiledPanelPublication(publication, capabilities);
}
export function bundleForWidget(publication: CompiledPanelPublication, widgetId: string): CompiledWidgetBundle {
  const widget = publication.document.presentation.widgets.find(widget => widget.id === widgetId); ensure(widget, 'Unknown compiled widget');
  const entry = publication.document.presentation.artifacts.find(entry => entry.hash === widget.artifactHash)!;
  return { artifact: entry.artifact, artifactHash: entry.hash, assets: publication.assets.filter(payload => entry.artifact.assets.some(asset => asset.id === payload.assetId)) };
}
export function panelControlValue(publication: CompiledPanelPublication, widgetId: string, portId: string, value: Scalar): { controlId: string; value: boolean | number } {
  const widget = publication.document.presentation.widgets.find(widget => widget.id === widgetId); ensure(widget, 'Unknown compiled widget');
  const binding = widget.inputBindings.find(binding => binding.portId === portId), port = bundleForWidget(publication, widgetId).artifact.inputs.find(port => port.id === portId); ensure(binding && port, 'Unknown compiled input port');
  return { controlId: binding.controlId, value: normalizedPortValue(port, value) };
}
export function panelFeedbackEvents(publication: CompiledPanelPublication, widgetId: string, samples: PanelFeedbackSamples): { event: string; feedback: Record<string, BehaviorFeedback> }[] {
  const widget = publication.document.presentation.widgets.find(widget => widget.id === widgetId); ensure(widget, 'Unknown compiled widget');
  const artifact = bundleForWidget(publication, widgetId).artifact, events = new Map<string, Record<string, BehaviorFeedback>>();
  for (const binding of widget.outputBindings) {
    const port = artifact.outputs.find(port => port.id === binding.portId)!, channel = publication.document.ui.feedbackWidgets.find(f => f.id === binding.feedbackId)!.channelId;
    const event = artifact.behavior.events.find(event => event.updates?.includes(port.signal))!.id;
    const sample = samples[channel] ?? { status: 'unknown' as const };
    const feedback = events.get(event) ?? {}; feedback[port.signal] = sample; events.set(event, feedback);
  }
  return [...events].map(([event, feedback]) => ({ event, feedback }));
}
