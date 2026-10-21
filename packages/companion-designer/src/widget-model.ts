import type { Bounds, FeedbackValue, Validation } from "../../companion-contracts/src/index.ts";

/** Portable source data. Neither LVGL objects nor browser markup belong in a definition. */
export const WIDGET_LIMITS = Object.freeze({ definitions: 64, layers: 32, properties: 24, ports: 8, bytes: 65536, libraryBytes: 1048576 });
export const WIDGET_STATES = ["normal", "active", "pressed", "warning", "stale", "unknown", "disabled", "disconnected"] as const;
export type WidgetState = typeof WIDGET_STATES[number];
export type WidgetPropertyValue = string | number | boolean;
export type WidgetProperty =
  | { name: string; label: string; type: "text" | "color" | "asset"; default: string }
  | { name: string; label: string; type: "boolean"; default: boolean }
  | { name: string; label: string; type: "number"; default: number; min: number; max: number };
export type WidgetValue<T> = T | { property: string };
export interface WidgetPaint {
  fill?: WidgetValue<string>; stroke?: WidgetValue<string>; strokeWidth?: WidgetValue<number>;
  opacity?: WidgetValue<number>; visible?: WidgetValue<boolean>;
}
export interface WidgetLayerBase extends WidgetPaint {
  id: string; label: string; bounds: Bounds;
  states?: Partial<Record<WidgetState, WidgetPaint & { text?: WidgetValue<string> }>>;
}
export type WidgetLayer =
  | (WidgetLayerBase & { kind: "rectangle"; radius?: WidgetValue<number> })
  | (WidgetLayerBase & { kind: "ellipse" | "line" })
  | (WidgetLayerBase & { kind: "text"; text: WidgetValue<string>; font: "sans" | "mono"; fontSize: number; align: "left" | "center" | "right"; outputPort?: string; precision?: number; unit?: string })
  | (WidgetLayerBase & { kind: "image"; assetId: WidgetValue<string> })
  | (WidgetLayerBase & { kind: "indicator"; outputPort: string })
  | (WidgetLayerBase & { kind: "gauge"; outputPort: string; min: WidgetValue<number>; max: WidgetValue<number>; orientation: "horizontal" | "vertical" | "radial" });
export type WidgetInputPort = { name: string; label: string } & (
  { kind: "button" } | { kind: "axis"; axisMode: "unipolar" | "bipolar"; fallback: number } | { kind: "hat"; fallback: -1 }
);
export interface WidgetOutputPort { name: string; label: string; kind: "indicator" | "text" | "gauge"; unit?: string; range?: { min: number; max: number } }
export type WidgetInteraction =
  | { kind: "none" }
  | { kind: "momentary" | "toggle"; port: string }
  | { kind: "selector"; port: string; options: { label: string; value: number }[] }
  | { kind: "navigate"; pageProperty: string };
export interface WidgetDefinition {
  formatVersion: 1; id: string; version: number; title: string;
  size: { width: number; height: number }; properties: WidgetProperty[];
  inputs: WidgetInputPort[]; outputs: WidgetOutputPort[]; interaction: WidgetInteraction;
  hitArea: Bounds; layers: WidgetLayer[];
  state: { confirmedPort?: string; warningPort?: string; enabledPort?: string };
}
export interface WidgetInstance {
  definitionId: string; definitionVersion: number; properties: Record<string, WidgetPropertyValue>;
}
export interface WidgetLibrary { formatVersion: 1; definitions: WidgetDefinition[] }
export type WidgetSample = FeedbackValue & { receivedAtMs: number };
export interface WidgetStateContext {
  connected: boolean; enabled: boolean; pressed: boolean; now: number;
  samples: Record<string, WidgetSample | undefined>;
}
export interface WidgetCapabilityProfile {
  id: string; features: readonly string[]; maxLayers: number; maxDefinitions: number;
  maxDocumentBytes: number; minTouchPixels: number; maxImagePixels: number;
}
export interface WidgetCompatibilityIssue { widgetId: string; feature: string; required: number | string; available: number | string; message: string }

function ensure(test: unknown, message: string): asserts test { if (!test) throw new Error(message); }
function record(value: unknown, required: string[], optional: string[] = []): Record<string, unknown> {
  ensure(value && typeof value === "object" && !Array.isArray(value) && Object.getPrototypeOf(value) === Object.prototype, "Expected a plain object");
  const item = value as Record<string, unknown>;
  ensure(required.every(key => Object.hasOwn(item, key)) && Object.keys(item).every(key => required.includes(key) || optional.includes(key)), "Unexpected or missing widget field");
  return item;
}
function stable(value: unknown): asserts value is string { ensure(typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value), "Invalid widget stable ID"); }
function text(value: unknown): asserts value is string { ensure(typeof value === "string" && value.length <= 120 && !/[\u0000-\u001f]/.test(value), "Invalid widget text"); }
function num(value: unknown, min: number, max: number): asserts value is number { ensure(typeof value === "number" && Number.isFinite(value) && value >= min && value <= max, "Widget number outside bounds"); }
function int(value: unknown, min: number, max: number): asserts value is number { num(value, min, max); ensure(Number.isSafeInteger(value), "Expected widget integer"); }
function list(value: unknown, max: number): unknown[] { ensure(Array.isArray(value) && value.length <= max, "Widget list exceeds limit"); return value; }
function bounds(value: unknown): void {
  const b = record(value, ["x", "y", "width", "height"]);
  for (const v of Object.values(b)) num(v, 0, 1);
  ensure(Number(b.width) > 0 && Number(b.height) > 0 && Number(b.x) + Number(b.width) <= 1 + 1e-9 && Number(b.y) + Number(b.height) <= 1 + 1e-9, "Widget bounds outside component");
}
function propertyValue(property: WidgetProperty, value: unknown, assets?: ReadonlySet<string>): void {
  switch (property.type) {
    case "boolean": ensure(typeof value === "boolean", "Invalid boolean widget property"); break;
    case "number": num(value, property.min, property.max); break;
    case "color": ensure(typeof value === "string" && /^#[a-fA-F0-9]{6}$/.test(value), "Invalid widget color"); break;
    case "asset": text(value); ensure(value === "" || !assets || assets.has(value), "Missing widget asset"); break;
    case "text": text(value); break;
  }
}
function value(value: unknown, type: "color" | "text" | "asset" | "number" | "boolean", properties: WidgetProperty[], assets?: ReadonlySet<string>, min = 0, max = 1): void {
  if (typeof value === "object") {
    const ref = record(value, ["property"]), property = properties.find(p => p.name === ref.property);
    ensure(property && property.type === type, "Incompatible widget property reference");
    if (property.type === "number") ensure(property.min >= min && property.max <= max, "Widget property range exceeds layer bounds");
  } else if (type === "number") num(value, min, max);
  else propertyValue(type === "boolean" ? { name: "", label: "", type, default: false } : { name: "", label: "", type, default: "" }, value, assets);
}
const paintKeys = ["fill", "stroke", "strokeWidth", "opacity", "visible"];
function paint(p: Record<string, unknown>, properties: WidgetProperty[], assets?: ReadonlySet<string>): void {
  for (const key of ["fill", "stroke"]) if (p[key] !== undefined) value(p[key], "color", properties, assets);
  if (p.strokeWidth !== undefined) value(p.strokeWidth, "number", properties, assets, 0, 16);
  if (p.opacity !== undefined) value(p.opacity, "number", properties, assets);
  if (p.visible !== undefined) value(p.visible, "boolean", properties, assets);
}
/** Strict source validator, also usable by the library importer before a document exists. */
export function validateWidgetDefinition(input: unknown, assetIds?: readonly string[]): Validation<WidgetDefinition> {
  try {
    const serialized = JSON.stringify(input);
    ensure(serialized && new TextEncoder().encode(serialized).length <= WIDGET_LIMITS.bytes, "Widget definition exceeds byte budget");
    const d = record(JSON.parse(serialized), ["formatVersion", "id", "version", "title", "size", "properties", "inputs", "outputs", "interaction", "hitArea", "layers", "state"]);
    ensure(d.formatVersion === 1, "Unsupported widget definition version"); stable(d.id); int(d.version, 1, 65535); text(d.title); ensure(String(d.title).trim(), "Widget title required");
    const size = record(d.size, ["width", "height"]); int(size.width, 32, 1024); int(size.height, 32, 1024); bounds(d.hitArea);
    const assets = assetIds ? new Set(assetIds) : undefined, propertyNames = new Set<string>();
    const properties = list(d.properties, WIDGET_LIMITS.properties) as WidgetProperty[];
    for (const p of properties) {
      record(p, ["name", "label", "type", "default"], p.type === "number" ? ["min", "max"] : []); stable(p.name); text(p.label);
      ensure(!propertyNames.has(p.name), "Duplicate widget property"); propertyNames.add(p.name);
      ensure(["text", "color", "number", "boolean", "asset"].includes(p.type), "Unsupported widget property type");
      if (p.type === "number") { num(p.min, -1e9, 1e9); num(p.max, -1e9, 1e9); ensure(p.min < p.max, "Invalid widget property range"); }
      propertyValue(p, p.default, assets);
    }
    const inputs = list(d.inputs, WIDGET_LIMITS.ports) as WidgetInputPort[], outputs = list(d.outputs, WIDGET_LIMITS.ports) as WidgetOutputPort[], portNames = new Set<string>();
    for (const p of [...inputs, ...outputs]) { stable(p.name); text(p.label); ensure(!portNames.has(p.name), "Duplicate widget port"); portNames.add(p.name); }
    for (const p of inputs) {
      record(p, ["name", "label", "kind"], p.kind === "axis" ? ["axisMode", "fallback"] : p.kind === "hat" ? ["fallback"] : []);
      ensure(["button", "axis", "hat"].includes(p.kind), "Unsupported widget input port");
      if (p.kind === "axis") { ensure(["unipolar", "bipolar"].includes(p.axisMode), "Invalid widget axis mode"); num(p.fallback, p.axisMode === "bipolar" ? -1 : 0, 1); }
      if (p.kind === "hat") ensure(p.fallback === -1, "Invalid widget hat fallback");
    }
    for (const p of outputs) {
      record(p, ["name", "label", "kind"], ["unit", "range"]); ensure(["indicator", "text", "gauge"].includes(p.kind), "Unsupported widget output port"); if (p.unit !== undefined) text(p.unit);
      if (p.range !== undefined) { record(p.range, ["min", "max"]); ensure(p.kind === "gauge", "Only numeric widget ports have ranges"); num(p.range.min, -1e9, 1e9); num(p.range.max, -1e9, 1e9); ensure(p.range.min < p.range.max, "Invalid widget output range"); }
    }
    const interaction = d.interaction as WidgetInteraction;
    record(interaction, ["kind"], interaction.kind === "selector" ? ["port", "options"] : interaction.kind === "navigate" ? ["pageProperty"] : ["momentary", "toggle"].includes(interaction.kind) ? ["port"] : []);
    ensure(["none", "momentary", "toggle", "selector", "navigate"].includes(interaction.kind), "Unsupported widget interaction");
    if (interaction.kind === "momentary" || interaction.kind === "toggle" || interaction.kind === "selector") {
      const port = inputs.find(p => p.name === interaction.port);
      ensure(inputs.length === 1 && port?.kind === (interaction.kind === "selector" ? "axis" : "button"), "Incompatible widget interaction port");
      if (interaction.kind === "selector") {
        ensure(port.kind === "axis", "Selector requires axis"); const options = list(interaction.options, 16) as { label: string; value: number }[];
        ensure(options.length >= 2, "Selector needs options"); const seen = new Set<number>();
        for (const o of options) { record(o, ["label", "value"]); text(o.label); num(o.value, port.axisMode === "bipolar" ? -1 : 0, 1); ensure(!seen.has(o.value), "Duplicate selector value"); seen.add(o.value); }
        ensure(seen.has(port.fallback), "Selector fallback needs an option");
      }
    } else {
      ensure(inputs.length === 0, "Presentation/navigation widgets cannot own gameplay inputs");
      if (interaction.kind === "navigate") ensure(properties.some(p => p.name === interaction.pageProperty && p.type === "text"), "Navigation needs a page property");
    }
    const state = record(d.state, [], ["confirmedPort", "warningPort", "enabledPort"]);
    for (const name of Object.values(state)) ensure(outputs.some(p => p.name === name && p.kind === "indicator"), "Widget state requires an indicator output");
    const layerIds = new Set<string>();
    for (const layer of list(d.layers, WIDGET_LIMITS.layers) as WidgetLayer[]) {
      const extra = layer.kind === "rectangle" ? ["radius"] : layer.kind === "text" ? ["text", "font", "fontSize", "align", "outputPort", "precision", "unit"] : layer.kind === "image" ? ["assetId"] : layer.kind === "indicator" ? ["outputPort"] : layer.kind === "gauge" ? ["outputPort", "min", "max", "orientation"] : [];
      record(layer, ["id", "label", "kind", "bounds"], [...paintKeys, "states", ...extra]); stable(layer.id); text(layer.label); bounds(layer.bounds);
      ensure(!layerIds.has(layer.id), "Duplicate widget layer"); layerIds.add(layer.id);
      ensure(["rectangle", "ellipse", "line", "text", "image", "indicator", "gauge"].includes(layer.kind), "Unsupported widget layer"); paint(layer as unknown as Record<string, unknown>, properties, assets);
      if (layer.kind === "rectangle" && layer.radius !== undefined) value(layer.radius, "number", properties, assets, 0, 64);
      if (layer.kind === "image") value(layer.assetId, "asset", properties, assets);
      if (layer.kind === "text") {
        value(layer.text, "text", properties); ensure(["sans", "mono"].includes(layer.font) && ["left", "center", "right"].includes(layer.align), "Unsupported widget typography"); int(layer.fontSize, 8, 128);
        if (layer.precision !== undefined) int(layer.precision, 0, 4); if (layer.unit !== undefined) text(layer.unit);
        if (layer.outputPort !== undefined) ensure(outputs.some(p => p.name === layer.outputPort), "Unknown widget text output");
      }
      if (layer.kind === "indicator" || layer.kind === "gauge") ensure(outputs.some(p => p.name === layer.outputPort && p.kind === (layer.kind === "indicator" ? "indicator" : "gauge")), "Incompatible widget layer output");
      if (layer.kind === "gauge") {
        value(layer.min, "number", properties, assets, -1e9, 1e9); value(layer.max, "number", properties, assets, -1e9, 1e9);
        ensure(["horizontal", "vertical", "radial"].includes(layer.orientation), "Unsupported gauge orientation");
        const minimum = layer.min, maximum = layer.max;
        const low = typeof minimum === "number" ? minimum : Number(properties.find(p => p.name === minimum.property)?.default);
        const high = typeof maximum === "number" ? maximum : Number(properties.find(p => p.name === maximum.property)?.default);
        ensure(low < high, "Invalid gauge range");
      }
      if (layer.states !== undefined) {
        const states = record(layer.states, [], [...WIDGET_STATES]);
        for (const overrides of Object.values(states)) {
          const p = record(overrides, [], [...paintKeys, ...(layer.kind === "text" ? ["text"] : [])]); paint(p, properties, assets);
          if (p.text !== undefined) value(p.text, "text", properties);
        }
      }
    }
    ensure(layerIds.size > 0, "Widget needs a visual layer");
    return { ok: true, value: d as unknown as WidgetDefinition };
  } catch (e) { return { ok: false, error: e instanceof Error ? e.message : "Invalid widget definition" }; }
}

export function resolveWidgetProperties(definition: WidgetDefinition, overrides: Record<string, WidgetPropertyValue>, assetIds?: readonly string[]): Record<string, WidgetPropertyValue> {
  record(overrides, [], definition.properties.map(p => p.name));
  const assets = assetIds ? new Set(assetIds) : undefined;
  return Object.fromEntries(definition.properties.map(p => { const v = Object.hasOwn(overrides, p.name) ? overrides[p.name] : p.default; propertyValue(p, v, assets); return [p.name, v]; }));
}
export function resolveWidgetValue<T extends WidgetPropertyValue>(value: WidgetValue<T>, properties: Record<string, WidgetPropertyValue>): T {
  return (typeof value === "object" ? properties[value.property] : value) as T;
}
/** Freshness is derived from source age plus elapsed receive time, never arrival alone. */
export function currentWidgetSample(sample: WidgetSample | undefined, now: number): WidgetSample | undefined {
  if (!sample || sample.status !== "fresh" || !Number.isFinite(now) || !Number.isFinite(sample.receivedAtMs) || !Number.isFinite(sample.sourceAgeMs) || sample.sourceAgeMs < 0 || sample.receivedAtMs > now || sample.sourceAgeMs + now - sample.receivedAtMs >= 1000) return undefined;
  return sample;
}
export function resolveWidgetState(definition: WidgetDefinition, context: WidgetStateContext): WidgetState {
  if (!context.connected) return "disconnected";
  if (!context.enabled) return "disabled";
  const sample = (name: string | undefined) => name ? currentWidgetSample(context.samples[name], context.now) : undefined;
  const gate = sample(definition.state.enabledPort);
  if (gate?.value === false) return "disabled";
  if (Object.values(definition.state).some(name => context.samples[name]?.status === "disconnected")) return "disconnected";
  if (sample(definition.state.warningPort)?.value === true) return "warning";
  // Local press feedback remains available before telemetry arrives. Indicator
  // layers independently require fresh game-confirmed samples to illuminate.
  if (context.pressed) return "pressed";
  for (const name of Object.values(definition.state)) {
    const s = context.samples[name];
    if (!s || s.status === "unknown" || s.status === "fresh" && typeof s.value !== "boolean") return "unknown";
  }
  if (Object.values(definition.state).some(name => !sample(name))) return "stale";
  const confirmed = sample(definition.state.confirmedPort);
  return confirmed?.value === true && confirmed.provenance === "game-confirmed" && confirmed.sourceKind === "simulator" ? "active" : "normal";
}
export function resolveWidgetLayers(definition: WidgetDefinition, overrides: Record<string, WidgetPropertyValue>, state: WidgetState): WidgetLayer[] {
  const properties = resolveWidgetProperties(definition, overrides);
  return definition.layers.map(original => {
    const { states, ...base } = original;
    const layer = { ...base, ...states?.normal, ...states?.[state] } as unknown as Record<string, unknown>;
    for (const [key, v] of Object.entries(layer)) if (v && typeof v === "object" && Object.hasOwn(v, "property")) layer[key] = properties[(v as { property: string }).property];
    if (layer.kind === "gauge") ensure(Number(layer.min) < Number(layer.max), "Invalid instance gauge range");
    return layer as unknown as WidgetLayer;
  });
}
/** Semantic identity deliberately excludes artwork, fonts, layout and labels. */
export function widgetSemantics(definition: WidgetDefinition, overrides: Record<string, WidgetPropertyValue> = {}): unknown {
  const properties = resolveWidgetProperties(definition, overrides);
  return {
    inputs: definition.inputs.map(({ label: _label, ...p }) => p),
    outputs: definition.outputs.map(({ label: _label, ...p }) => p),
    interaction: definition.interaction,
    ...(definition.interaction.kind === "navigate" ? { page: properties[definition.interaction.pageProperty] } : {}),
  };
}
export function widgetRequiredFeatures(definition: WidgetDefinition): string[] {
  return [...new Set(["widget.v1", `interaction.${definition.interaction.kind}`, ...definition.layers.map(l => `layer.${l.kind === "gauge" ? `gauge.${l.orientation}` : l.kind}`), ...definition.layers.filter(l => l.kind === "text").map(l => `font.${l.font}`)])].sort();
}
export function widgetCompatibility(definition: WidgetDefinition, profile: WidgetCapabilityProfile, widgetId: string, width: number, height: number): WidgetCompatibilityIssue[] {
  const issues: WidgetCompatibilityIssue[] = [];
  const add = (feature: string, required: number | string, available: number | string) => issues.push({ widgetId, feature, required, available, message: `Widget ${widgetId} requires ${feature}: ${required}; ${profile.id} provides ${available}.` });
  for (const feature of widgetRequiredFeatures(definition)) if (!profile.features.includes(feature)) add(feature, "supported", "unavailable");
  if (definition.layers.length > profile.maxLayers) add("layers", definition.layers.length, profile.maxLayers);
  if (definition.interaction.kind !== "none") {
    const pixels = Math.floor(Math.min(width * definition.hitArea.width, height * definition.hitArea.height));
    if (pixels < profile.minTouchPixels) add("touchPixels", profile.minTouchPixels, pixels);
  }
  return issues;
}
export function validateWidgetLibrary(input: unknown): Validation<WidgetLibrary> {
  try {
    const serialized = JSON.stringify(input); ensure(serialized && new TextEncoder().encode(serialized).length <= WIDGET_LIMITS.libraryBytes, "Widget library exceeds budget");
    const library = record(JSON.parse(serialized), ["formatVersion", "definitions"]); ensure(library.formatVersion === 1, "Unsupported widget library version");
    const seen = new Set<string>();
    const definitions = list(library.definitions, WIDGET_LIMITS.definitions).map(input => {
      const result = validateWidgetDefinition(input); ensure(result.ok, result.ok ? "" : result.error);
      const key = `${result.value.id}@${result.value.version}`; ensure(!seen.has(key), "Duplicate widget library revision"); seen.add(key); return result.value;
    });
    return { ok: true, value: { formatVersion: 1, definitions } };
  } catch (e) { return { ok: false, error: e instanceof Error ? e.message : "Invalid widget library" }; }
}
export function saveWidgetDefinition(library: WidgetLibrary, definition: WidgetDefinition): WidgetLibrary {
  const validated = validateWidgetLibrary(library); ensure(validated.ok, validated.ok ? "" : validated.error);
  const item = validateWidgetDefinition(definition); ensure(item.ok, item.ok ? "" : item.error);
  ensure(!library.definitions.some(d => d.id === item.value.id && d.version === item.value.version), "Widget revisions are immutable; save a new version");
  const next = validateWidgetLibrary({ formatVersion: 1, definitions: [...validated.value.definitions, item.value] }); ensure(next.ok, next.ok ? "" : next.error); return next.value;
}
