import { COMPANION_LIMITS, validatePortableUi } from "../../companion-contracts/src/index.ts";
import type { Bounds, Control, PortableUi, Style, Validation } from "../../companion-contracts/src/index.ts";
import { validateAssets } from "./assets.ts";
import { assert, canonical, checked, clone, digest, exact, integer, label, number, stableId } from "./model.ts";
import { DESIGNER_LIMITS } from "./types.ts";
import type { AssetPayload, DesignerGroup, DesignerWidget, LogicalBinding, LogicalTarget, Viewport } from "./types.ts";

/** Explicit authoring path. Schema 2 readers, hashes, stores and publications do
 * not consume or automatically migrate this model. Recipes contain declarative
 * visual data resolved by a future catalog/compiler, never executable scripts. */
export type CompositeInteraction =
  | { kind: "display" | "momentary" | "toggle" }
  | { kind: "guardedMomentary"; guardMs: number }
  | { kind: "navigation"; destinationPage: string }
  | { kind: "xy"; centering: "spring" | "latched"; deadzone: number; invertX: boolean; invertY: boolean; atomic: true; reset: "fallback" }
  | { kind: "barycentric3"; vertices: [number, number][]; projection: "closest-point"; quantizationSteps: number; atomic: true; reset: "fallback" };
export interface CompositeOwner { id: string; controlIds: string[]; feedbackIds: string[]; interaction: CompositeInteraction }
export interface CompositeFeedbackContract {
  outputId: string; valueType: "boolean" | "number" | "text"; unit: string; provenancePolicy: "preserve-source";
  range?: { min: number; max: number }; maxTextBytes?: number;
}
export interface CompositeRecipeReference {
  id: string; version: number; sha256: string; targetProfileIds: string[]; requires: string[];
}
export interface CompositeReference { slot: string; kind: "control" | "feedback"; refId: string }
export interface CompositeComponent {
  id: string; pageId: string; label: string; bounds: Bounds; ownerIds: string[];
  recipe: CompositeRecipeReference; references: CompositeReference[]; parameters: Record<string, string | number | boolean>;
  assetIds: string[]; groupId?: string; style?: Style;
}
export interface DesignerDocumentV3 {
  schemaVersion: 3; ui: PortableUi;
  semantics: { version: 1; owners: CompositeOwner[]; feedback: CompositeFeedbackContract[] };
  presentation: { components: CompositeComponent[]; groups: DesignerGroup[]; viewports: Viewport[]; grid: { columns: number; rows: number; snap: boolean } };
  targets: LogicalTarget[]; bindings: LogicalBinding[];
}
export interface CompositePublication { document: DesignerDocumentV3; assets: AssetPayload[]; publicationHash: string; capabilityFingerprint: string }
export interface CompositeTargetDiagnostic { componentId: string; targetProfileId: string; requiredFeature?: string; message: string }
const encoder = new TextEncoder();
const featurePattern = /^[a-z][a-z0-9.-]{0,95}$/;
const interactionFeatures: Record<CompositeInteraction["kind"], string | undefined> = {
  display: undefined, momentary: "input.momentary", toggle: "input.toggle", guardedMomentary: "input.guarded-momentary",
  navigation: "input.navigation", xy: "input.xy", barycentric3: "input.barycentric3",
};
function list(value: unknown, min: number, max: number): unknown[] { assert(Array.isArray(value) && value.length >= min && value.length <= max, "Array outside composite limits"); return value; }
function ids(value: unknown, max: number): string[] {
  const values = list(value, 0, max); const seen = new Set<string>();
  for (const item of values) { stableId(item); assert(!seen.has(item), "Duplicate composite reference"); seen.add(item); }
  return values as string[];
}
function bounds(value: unknown): void {
  const b = exact(value, ["x", "y", "width", "height"]);
  for (const coordinate of Object.values(b)) number(coordinate, 0, 1);
  assert(Number(b.width) > 0 && Number(b.height) > 0 && Number(b.x) + Number(b.width) <= 1 && Number(b.y) + Number(b.height) <= 1, "Composite bounds outside page");
}
function style(value: unknown): void {
  const item = exact(value, [], ["foreground", "background", "accent"]);
  for (const color of Object.values(item)) assert(typeof color === "string" && /^#[a-fA-F0-9]{6}$/.test(color), "Invalid composite color");
}
function jsonData(value: unknown, depth = 0): void {
  assert(depth <= 32, "Composite JSON nesting exceeds limit");
  if (value === null || typeof value === "string" || typeof value === "boolean") return;
  if (typeof value === "number") { assert(Number.isFinite(value), "Nonfinite composite number"); return; }
  assert(value && typeof value === "object" && (Array.isArray(value) || Object.getPrototypeOf(value) === Object.prototype), "Composite source requires plain JSON data");
  for (const child of Object.values(value)) jsonData(child, depth + 1);
}
function interaction(owner: CompositeOwner, controls: Control[], pages: Set<string>): void {
  const value = owner.interaction;
  assert(value && Object.hasOwn(interactionFeatures, value.kind), "Unsupported composite interaction");
  switch (value.kind) {
    case "display": exact(value, ["kind"]); assert(controls.length === 0 && owner.feedbackIds.length > 0, "Display owner needs feedback and no controls"); return;
    case "navigation": exact(value, ["kind", "destinationPage"]); assert(controls.length === 0 && pages.has(value.destinationPage), "Invalid navigation destination or controls"); return;
    case "momentary": case "toggle": exact(value, ["kind"]); break;
    case "guardedMomentary": exact(value, ["kind", "guardMs"]); integer(value.guardMs, 1, 60000); break;
    case "xy":
      exact(value, ["kind", "centering", "deadzone", "invertX", "invertY", "atomic", "reset"]);
      assert(controls.length === 2 && controls.every(control => control.kind === "axis" && control.axisMode === "bipolar"), "XY requires two bipolar axes");
      assert(value.centering === "spring" || value.centering === "latched", "Invalid XY centering"); number(value.deadzone, 0, 0.49);
      assert(typeof value.invertX === "boolean" && typeof value.invertY === "boolean" && value.atomic === true && value.reset === "fallback", "Invalid XY transform or atomic policy"); return;
    case "barycentric3": {
      exact(value, ["kind", "vertices", "projection", "quantizationSteps", "atomic", "reset"]);
      assert(controls.length === 3 && controls.every(control => control.kind === "axis" && control.axisMode === "unipolar"), "Triangle requires three unipolar axes");
      const vertices = list(value.vertices, 3, 3).map(vertex => list(vertex, 2, 2).map(coordinate => { number(coordinate, 0, 1); return coordinate; }));
      const [a, b, c] = vertices;
      assert(Math.abs((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) >= 1e-6, "Degenerate power triangle");
      integer(value.quantizationSteps, 1, 65535);
      assert(value.projection === "closest-point" && value.atomic === true && value.reset === "fallback", "Invalid triangle projection or atomic policy");
      const sum = controls.reduce((sum, control) => sum + (control.kind === "axis" ? control.fallback : 0), 0);
      assert(Math.abs(sum - 1) <= 1 / value.quantizationSteps, "Triangle fallback must sum to one within quantization tolerance"); return;
    }
  }
  assert(controls.length === 1 && controls[0].kind === "button", "Button interaction requires one button");
}
function logicalTargets(doc: DesignerDocumentV3, newId: (id: unknown) => void): void {
  for (const value of list(doc.targets, 0, DESIGNER_LIMITS.targets)) {
    const target = exact(value, ["id", "label", "axes", "buttons", "hats"]); newId(target.id); label(target.label);
    for (const axis of list(target.axes, 0, 16)) assert(axis === "unipolar" || axis === "bipolar", "Invalid logical target axis");
    integer(target.buttons, 0, 128); integer(target.hats, 0, 4);
    assert((target.axes as unknown[]).length + target.buttons + target.hats > 0, "Empty logical target");
  }
  const bound = new Set<string>(), slots = new Set<string>();
  for (const value of list(doc.bindings, 0, doc.ui.controls.length)) {
    const binding = exact(value, ["controlId", "targetId", "kind", "index"]);
    const control = doc.ui.controls.find(control => control.id === binding.controlId), target = doc.targets.find(target => target.id === binding.targetId);
    assert(control && target && binding.kind === control.kind && !bound.has(control.id), "Invalid composite logical binding");
    integer(binding.index, 0, control.kind === "axis" ? target.axes.length - 1 : control.kind === "button" ? target.buttons - 1 : target.hats - 1);
    if (control.kind === "axis") assert(target.axes[binding.index] === control.axisMode, "Target axis range mismatch");
    const slot = target.id + ":" + control.kind + ":" + binding.index; assert(!slots.has(slot), "Target slot already bound"); slots.add(slot); bound.add(control.id);
  }
}
/** PortableUi1 stays a single logical projection. Decorative/read-only aliases
 * use recipe references, never extra page placements or extra logical owners. */
export function normalizeCompositeToPortableUi(document: DesignerDocumentV3): PortableUi {
  const ui = clone(document.ui);
  ui.pages = ui.pages.map(page => ({ ...page, items: document.presentation.components.filter(component => component.pageId === page.id).flatMap(component =>
    component.ownerIds.flatMap(ownerId => {
      const owner = document.semantics.owners.find(owner => owner.id === ownerId); assert(owner, "Unknown component owner");
      return [...owner.controlIds.map(refId => ({ kind: "control" as const, refId, bounds: clone(component.bounds) })),
        ...owner.feedbackIds.map(refId => ({ kind: "feedback" as const, refId, bounds: clone(component.bounds) }))];
    })) }));
  return ui;
}
export function validateDesignerDocumentV3(input: unknown): Validation<DesignerDocumentV3> {
  try {
    jsonData(input);
    const serialized = JSON.stringify(input); assert(encoder.encode(serialized).length <= 1024 * 1024, "Composite document exceeds 1 MiB");
    const raw = exact(JSON.parse(serialized), ["schemaVersion", "ui", "semantics", "presentation", "targets", "bindings"]);
    assert(raw.schemaVersion === 3, "Unsupported composite designer schema");
    const logical = validatePortableUi(raw.ui); assert(logical.ok, logical.ok ? "" : logical.error);
    const doc = raw as unknown as DesignerDocumentV3, ui = logical.value;
    const pages = new Set(ui.pages.map(page => page.id));
    const used = new Set([...ui.controls.map(control => control.id), ...ui.feedbackWidgets.map(output => output.id), ...pages, ...ui.assets.map(asset => asset.id)]);
    const newId = (id: unknown) => { stableId(id); assert(!used.has(id), "Duplicate composite stable ID"); used.add(id); };
    const semantics = exact(doc.semantics, ["version", "owners", "feedback"]); assert(semantics.version === 1, "Unsupported semantic descriptor version");
    const ownedControls = new Set<string>(), ownedFeedback = new Set<string>();
    for (const item of list(semantics.owners, 1, DESIGNER_LIMITS.widgets)) {
      const owner = exact(item, ["id", "controlIds", "feedbackIds", "interaction"]) as unknown as CompositeOwner; newId(owner.id);
      const controls = ids(owner.controlIds, COMPANION_LIMITS.controls).map(id => {
        const control = ui.controls.find(control => control.id === id); assert(control && !ownedControls.has(id), "Unknown or multiply owned control"); ownedControls.add(id); return control;
      });
      for (const id of ids(owner.feedbackIds, COMPANION_LIMITS.feedbackWidgets)) { assert(ui.feedbackWidgets.some(output => output.id === id) && !ownedFeedback.has(id), "Unknown or multiply owned feedback"); ownedFeedback.add(id); }
      interaction(owner, controls, pages);
    }
    assert(ownedControls.size === ui.controls.length && ownedFeedback.size === ui.feedbackWidgets.length, "Unowned logical primitive");
    const contracted = new Set<string>();
    for (const value of list(semantics.feedback, ui.feedbackWidgets.length, ui.feedbackWidgets.length)) {
      const contract = exact(value, ["outputId", "valueType", "unit", "provenancePolicy"], ["range", "maxTextBytes"]);
      const output = ui.feedbackWidgets.find(output => output.id === contract.outputId);
      assert(output && !contracted.has(output.id), "Unknown or duplicate feedback contract"); contracted.add(output.id);
      assert(contract.valueType === (output.kind === "gauge" ? "number" : output.kind === "indicator" ? "boolean" : "text"), "Feedback contract type mismatch");
      label(contract.unit, true); assert(contract.provenancePolicy === "preserve-source", "Invalid feedback provenance policy");
      if (contract.valueType === "number") { const range = exact(contract.range, ["min", "max"]); number(range.min, -1e9, 1e9); number(range.max, -1e9, 1e9); assert(range.min < range.max && contract.maxTextBytes === undefined, "Invalid numeric feedback contract"); }
      else { assert(contract.range === undefined && contract.unit === "", "Non-numeric feedback cannot declare range or unit"); if (contract.valueType === "text") integer(contract.maxTextBytes, 1, 1024); else assert(contract.maxTextBytes === undefined, "Boolean feedback cannot declare text capacity"); }
    }
    const p = exact(doc.presentation, ["components", "groups", "viewports", "grid"]);
    for (const value of list(p.groups, 0, DESIGNER_LIMITS.groups)) { const group = exact(value, ["id", "pageId", "label"]); newId(group.id); label(group.label); assert(pages.has(String(group.pageId)), "Unknown group page"); }
    const grid = exact(p.grid, ["columns", "rows", "snap"]); integer(grid.columns, 1, 128); integer(grid.rows, 1, 128); assert(typeof grid.snap === "boolean", "Invalid composite grid");
    const viewports = new Set<string>();
    for (const value of list(p.viewports, pages.size, pages.size)) {
      const viewport = exact(value, ["pageId", "width", "height"], ["safeInsets", "backgroundAssetId"]);
      assert(pages.has(String(viewport.pageId)) && !viewports.has(String(viewport.pageId)), "Invalid viewport page"); viewports.add(String(viewport.pageId));
      integer(viewport.width, 320, 8192); integer(viewport.height, 320, 8192);
      if (viewport.backgroundAssetId !== undefined) assert(ui.assets.some(asset => asset.id === viewport.backgroundAssetId), "Missing viewport background asset");
      if (viewport.safeInsets !== undefined) { const inset = exact(viewport.safeInsets, ["top", "right", "bottom", "left"]); for (const value of Object.values(inset)) number(value, 0, 0.25); assert(Number(inset.left) + Number(inset.right) < 0.5 && Number(inset.top) + Number(inset.bottom) < 0.5, "Invalid composite safe insets"); }
    }
    const placed = new Set<string>();
    for (const value of list(p.components, 1, DESIGNER_LIMITS.widgets)) {
      const component = exact(value, ["id", "pageId", "label", "bounds", "ownerIds", "recipe", "references", "parameters", "assetIds"], ["groupId", "style"]) as unknown as CompositeComponent;
      newId(component.id); label(component.label); bounds(component.bounds); assert(pages.has(component.pageId), "Unknown component page");
      if (component.style !== undefined) style(component.style);
      if (component.groupId !== undefined) assert(doc.presentation.groups.some(group => group.id === component.groupId && group.pageId === component.pageId), "Invalid component group");
      const primitiveIds = new Set<string>();
      for (const id of ids(component.ownerIds, DESIGNER_LIMITS.widgets)) { const owner = doc.semantics.owners.find(owner => owner.id === id); assert(owner && !placed.has(id), "Unknown or multiply placed owner"); placed.add(id); for (const primitive of [...owner.controlIds, ...owner.feedbackIds]) primitiveIds.add(primitive); }
      const recipe = exact(component.recipe, ["id", "version", "sha256", "targetProfileIds", "requires"]); stableId(recipe.id); integer(recipe.version, 1, 65535);
      assert(typeof recipe.sha256 === "string" && /^sha256:[a-f0-9]{64}$/.test(recipe.sha256), "Recipe must name an immutable SHA-256 digest");
      assert(ids(recipe.targetProfileIds, 32).length > 0, "Recipe needs explicit compatible targets");
      const required = list(recipe.requires, 0, 64); assert(required.every(feature => typeof feature === "string" && featurePattern.test(feature)) && new Set(required).size === required.length, "Invalid recipe feature set");
      const slots = new Set<string>(), referenced = new Set<string>();
      for (const item of list(component.references, 0, 256)) {
        const ref = exact(item, ["slot", "kind", "refId"]); stableId(ref.slot); stableId(ref.refId); assert(!slots.has(ref.slot), "Duplicate recipe slot"); slots.add(ref.slot);
        assert(ref.kind === "control" ? primitiveIds.has(ref.refId) && ui.controls.some(control => control.id === ref.refId) : ref.kind === "feedback" && ui.feedbackWidgets.some(output => output.id === ref.refId), "Unknown or unowned recipe reference");
        referenced.add(ref.refId);
      }
      assert([...primitiveIds].every(id => referenced.has(id)), "Owned primitive missing a recipe reference");
      const parameters = exact(component.parameters, Object.keys(component.parameters)); assert(Object.keys(parameters).length <= 32, "Too many recipe parameters");
      for (const [key, parameter] of Object.entries(parameters)) { stableId(key); if (typeof parameter === "number") number(parameter, -1e9, 1e9); else assert(typeof parameter === "boolean" || typeof parameter === "string" && parameter.length <= 256 && !/[\u0000-\u001f]/.test(parameter), "Recipe parameters must be bounded scalar visual data"); }
      for (const id of ids(component.assetIds, 64)) assert(ui.assets.some(asset => asset.id === id), "Missing component asset");
    }
    assert(placed.size === doc.semantics.owners.length, "Unplaced semantic owner");
    logicalTargets(doc, newId);
    assert(canonical(normalizeCompositeToPortableUi(doc).pages) === canonical(ui.pages), "Composite and logical page projection differ");
    return { ok: true, value: doc };
  } catch (error) { return { ok: false, error: error instanceof Error ? error.message : "Invalid composite document" }; }
}
function checkedV3(document: DesignerDocumentV3): DesignerDocumentV3 {
  const result = validateDesignerDocumentV3(document); assert(result.ok, result.ok ? "" : result.error); return result.value;
}
function ordered<T extends { id: string }>(values: T[]): T[] { return values.sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0); }
/** Versioned mapping identity includes typed feedback and interaction ownership,
 * transforms, fallbacks and atomic groups, excluding all visual recipe data. */
export function compositeSemanticData(document: DesignerDocumentV3): unknown {
  const doc = checkedV3(document);
  return { schemaVersion: 3, fingerprintVersion: 1, uiId: doc.ui.id,
    controls: ordered(doc.ui.controls.map(control => ({ id: control.id, kind: control.kind, ...(control.kind === "axis" ? { axisMode: control.axisMode, fallback: control.fallback } : control.kind === "hat" ? { fallback: control.fallback } : {}) }))),
    feedback: ordered(doc.ui.feedbackWidgets.map(output => ({ id: output.id, kind: output.kind, channelId: output.channelId, contract: doc.semantics.feedback.find(contract => contract.outputId === output.id)! }))),
    owners: ordered(doc.semantics.owners), targets: ordered(doc.targets.map(({ id, axes, buttons, hats }) => ({ id, axes, buttons, hats }))),
    bindings: [...doc.bindings].sort((a, b) => a.controlId < b.controlId ? -1 : a.controlId > b.controlId ? 1 : 0) };
}
export function compareDesignerV3Publications(previous: DesignerDocumentV3, next: DesignerDocumentV3): "identical" | "styling" | "capability" {
  const first = checkedV3(previous), second = checkedV3(next);
  if (canonical(first) === canonical(second)) return "identical";
  return canonical(compositeSemanticData(first)) === canonical(compositeSemanticData(second)) ? "styling" : "capability";
}
function freeze<T>(value: T): T { if (value && typeof value === "object") { for (const child of Object.values(value)) freeze(child); Object.freeze(value); } return value; }
export async function publishDesignerV3Document(document: DesignerDocumentV3, assets: AssetPayload[]): Promise<CompositePublication> {
  const doc = checkedV3(document); assert(doc.bindings.length === doc.ui.controls.length, "Every input requires a logical target binding before publication");
  /* Shared asset validator only consumes ui.assets. Its legacy-shaped carrier
   * avoids changing the schema 2 validator, publication or hashing paths. */
  const payloads = await validateAssets({ schemaVersion: 2, ui: doc.ui, targets: [], bindings: [], presentation: { widgets: [], groups: [], viewports: [], grid: { columns: 1, rows: 1, snap: false } } }, assets);
  return freeze({ document: doc, assets: payloads, publicationHash: "sha256:" + await digest(encoder.encode(canonical(doc))),
    capabilityFingerprint: "sha256:" + await digest(encoder.encode(canonical(compositeSemanticData(doc)))) });
}
export function checkCompositeTarget(document: DesignerDocumentV3, profile: { id: string; features: readonly string[]; rendererAbi?: number; limits?: { dynamicStringBytes?: number } }): CompositeTargetDiagnostic[] {
  const doc = checkedV3(document), errors: CompositeTargetDiagnostic[] = [];
  for (const component of doc.presentation.components) {
    if (!component.recipe.targetProfileIds.includes(profile.id)) errors.push({ componentId: component.id, targetProfileId: profile.id, message: `Component ${component.id}: recipe ${component.recipe.id} does not support target ${profile.id}` });
    const features = new Set(component.recipe.requires);
    for (const id of component.ownerIds) {
      const owner = doc.semantics.owners.find(owner => owner.id === id)!, interaction = owner.interaction;
      const feature = interactionFeatures[interaction.kind]; if (feature) features.add(feature);
      const loweringError = (message: string) => errors.push({ componentId: component.id, targetProfileId: profile.id, message: `Component ${component.id}: ${message}` });
      if (profile.rendererAbi === 1) {
        if (interaction.kind === "guardedMomentary" && (interaction.guardMs < 50 || interaction.guardMs > 10000)) loweringError("render ABI 1 guard interval must be 50..10000 ms");
        if (interaction.kind === "xy") loweringError("render ABI 1 cannot encode the source XY transform/reset policy; a descriptor extension is required");
        if (interaction.kind === "barycentric3") {
          if (interaction.quantizationSteps !== 32767) loweringError("render ABI 1 triangle quantization requires 32767 steps");
          loweringError("render ABI 1 cannot encode the source triangle vertices/reset policy; a descriptor extension is required");
        }
      }
      for (const outputId of owner.feedbackIds) {
        const contract = doc.semantics.feedback.find(contract => contract.outputId === outputId)!;
        if (profile.rendererAbi === 1) {
          const unitBytes = encoder.encode(contract.unit).length;
          if (unitBytes > 24) loweringError(`feedback ${outputId} requires ${unitBytes} UTF-8 unit bytes; render ABI 1 allows 24`);
          if (contract.range) {
            // Match the renderer's checked, truncating signed Q16.16 conversion.
            const min = Math.trunc(contract.range.min * 65536), max = Math.trunc(contract.range.max * 65536);
            if (min < -2147483648 || max > 2147483647) loweringError(`feedback ${outputId} range exceeds signed Q16.16 limits in render ABI 1`);
            else if (min >= max) loweringError(`feedback ${outputId} range collapses during render ABI 1 Q16.16 conversion`);
          }
        }
        if (contract.maxTextBytes !== undefined && profile.limits?.dynamicStringBytes !== undefined && contract.maxTextBytes > profile.limits.dynamicStringBytes)
          loweringError(`feedback ${outputId} requires ${contract.maxTextBytes} text bytes; target allows ${profile.limits.dynamicStringBytes}`);
      }
    }
    for (const feature of features) if (!profile.features.includes(feature)) errors.push({ componentId: component.id, targetProfileId: profile.id, requiredFeature: feature, message: `Component ${component.id}: target ${profile.id} lacks ${feature}` });
  }
  return errors;
}
/** Migration is explicit, deterministic and requires actual catalog references.
 * Unsupported legacy behavior is an error, never removed or reinterpreted. */
export function migrateDesignerV2ToV3(input: unknown, resolveRecipe: (widget: DesignerWidget) => CompositeRecipeReference): DesignerDocumentV3 {
  const result = importV2(input), ui = clone(result.ui);
  const used = new Set([...ui.controls.map(control => control.id), ...ui.feedbackWidgets.map(output => output.id), ...ui.pages.map(page => page.id), ...ui.assets.map(asset => asset.id), ...result.targets.map(target => target.id), ...result.presentation.groups.map(group => group.id), ...result.presentation.widgets.map(widget => widget.id)]);
  const owners: CompositeOwner[] = [], feedback: CompositeFeedbackContract[] = [], components: CompositeComponent[] = [];
  for (const widget of result.presentation.widgets) {
    let behavior: CompositeInteraction;
    if (widget.kind === "button") { assert(widget.config.buttonMode === "momentary" || widget.config.buttonMode === "toggle", `Cannot migrate widget ${widget.id}: unsupported ${widget.config.buttonMode} behavior`); behavior = { kind: widget.config.buttonMode }; }
    else { assert(widget.controlIds.length === 0, `Cannot migrate widget ${widget.id}: unsupported ${widget.kind} interaction`); behavior = { kind: "display" }; }
    let ownerId = ("owner-" + widget.id).slice(0, 64), suffix = 0;
    while (used.has(ownerId)) ownerId = ("owner-" + widget.id).slice(0, 54) + "-" + ++suffix;
    used.add(ownerId);
    owners.push({ id: ownerId, controlIds: clone(widget.controlIds), feedbackIds: clone(widget.feedbackIds), interaction: behavior });
    for (const outputId of widget.feedbackIds) {
      const output = ui.feedbackWidgets.find(output => output.id === outputId)!;
      const contract: CompositeFeedbackContract = { outputId, valueType: output.kind === "gauge" ? "number" : output.kind === "indicator" ? "boolean" : "text", unit: "", provenancePolicy: "preserve-source" };
      if (widget.kind === "numeric" || widget.kind === "gauge") { contract.range = { min: widget.config.min, max: widget.config.max }; contract.unit = widget.config.unit; }
      /* Schema 2 permits 256 UTF-16 code units. Preserve a conservative UTF-8
       * capacity rather than silently shrinking it to an embedded profile cap. */
      if (output.kind === "text") contract.maxTextBytes = 1024;
      feedback.push(contract);
    }
    const parameters: CompositeComponent["parameters"] = {};
    if (widget.kind === "text") { parameters.prefix = widget.config.prefix; parameters.suffix = widget.config.suffix; }
    if (widget.kind === "numeric" || widget.kind === "gauge") parameters.precision = widget.config.precision;
    if (widget.kind === "icon") parameters.symbol = widget.config.symbol;
    const assetIds = (widget.kind === "button" || widget.kind === "icon") && widget.config.iconAssetId ? [widget.config.iconAssetId] : [];
    components.push({ id: widget.id, pageId: widget.pageId, label: widget.label, bounds: clone(widget.bounds), ownerIds: [ownerId], recipe: clone(resolveRecipe(clone(widget))),
      references: [...widget.controlIds.map((refId, index) => ({ slot: "control-" + index, kind: "control" as const, refId })), ...widget.feedbackIds.map((refId, index) => ({ slot: "feedback-" + index, kind: "feedback" as const, refId }))],
      parameters, assetIds, ...(widget.groupId ? { groupId: widget.groupId } : {}), ...(widget.style ? { style: clone(widget.style) } : {}) });
  }
  return checkedV3({ schemaVersion: 3, ui, semantics: { version: 1, owners, feedback }, presentation: { components, groups: clone(result.presentation.groups), viewports: clone(result.presentation.viewports), grid: clone(result.presentation.grid) }, targets: clone(result.targets), bindings: clone(result.bindings) });
}
function importV2(input: unknown) { return checked(input as Parameters<typeof checked>[0]); }
