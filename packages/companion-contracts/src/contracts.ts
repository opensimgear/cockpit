/** Shared companion contracts. These limits do not qualify a native/network backend. */
export const COMPANION_LIMITS = Object.freeze({
  clients: 8, targets: 8, roles: 16, controls: 128, pages: 16, feedbackWidgets: 64,
  uiBytes: 1024 * 1024, assetBytes: 2 * 1024 * 1024, totalAssetBytes: 8 * 1024 * 1024,
  frameBytes: 16 * 1024, feedbackBytes: 128 * 1024, changesPerBatch: 64, inputBatchesPerSecond: 120,
  feedbackFreshMs: 250, feedbackStaleMs: 1000, feedbackHz: 30, queuedTransitions: 512, heartbeatMs: 250, leaseMs: 1500, sessionIdsPerRun: 512,
});
export type Value = number | boolean;
export type Values = Record<string, Value>;
export interface Bounds { x: number; y: number; width: number; height: number }
export interface Style { foreground?: string; background?: string; accent?: string }
interface ControlBase { id: string; label: string; style?: Style }
export type Control =
  | (ControlBase & { kind: "button" })
  | (ControlBase & { kind: "axis"; axisMode: "bipolar" | "unipolar"; fallback: number })
  | (ControlBase & { kind: "hat"; fallback: -1 });
export interface FeedbackWidget { id: string; label: string; channelId: string; kind: "text" | "indicator" | "gauge"; style?: Style }
export interface PortableUi {
  schemaVersion: 1; id: string; title: string; controls: Control[];
  pages: { id: string; title: string; items: { kind: "control" | "feedback"; refId: string; bounds: Bounds }[] }[];
  feedbackWidgets: FeedbackWidget[];
  assets: { id: string; sha256: string; sizeBytes: number; mime: "image/png" | "image/jpeg" | "image/webp" }[];
}
export interface Hello { version: 1; type: "hello"; clientId: string; sessionId: string; generation: string; publicationHash: string; hostId: string; companionId: string; controllerInstanceId: string }
export interface InputBatch {
  version: 1; type: "input"; clientId: string; sessionId: string; generation: string;
  sequence: string; publicationHash: string; changes: { controlId: string; value: Value }[];
}
export interface FeedbackFrame {
  version: 1; type: "feedback"; clientId: string; sessionId: string; generation: string;
  sequence: string; publicationHash: string; values: FeedbackValue[];
}
export interface FeedbackUpdateFrame extends FeedbackFrame {
  controllerInstanceId: string; bindingGeneration: string; mode: "snapshot" | "delta";
  bindings?: { channelId: string; outputId: string }[];
}
export type FeedbackStatus = "fresh" | "unknown" | "stale" | "disconnected";
export interface FeedbackValue { channelId: string; outputId: string; status: FeedbackStatus; value?: Value | string; provenance: "requested" | "local" | "host-accepted" | "host-submitted" | "game-confirmed"; sourceKind: "simulator" | "host" | "script"; sourceAgeMs: number; unit: string; range?: { min: number; max: number } }
export interface HeartbeatFrame { version: 1; type: "heartbeat"; clientId: string; sessionId: string; generation: string; publicationHash: string; sequence: string }
export interface BaselineFrame { version: 1; type: "baseline"; clientId: string; sessionId: string; generation: string; publicationHash: string; sequence: "0"; values: Values }
export type Validation<T> = { ok: true; value: T } | { ok: false; error: string };
const idPattern = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;
const hashPattern = /^sha256:[a-f0-9]{64}$/;
const maxCounter = 18446744073709551615n;
const encoder = new TextEncoder();
function reject(message: string): never { throw new Error(message); }
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.getPrototypeOf(value) !== Object.prototype) reject("Expected a plain object");
  return value as Record<string, unknown>;
}
function keys(value: Record<string, unknown>, required: string[], optional: string[] = []): void {
  if (required.some(key => !Object.hasOwn(value, key)) || Object.keys(value).some(key => !required.includes(key) && !optional.includes(key))) reject("Unexpected or missing field");
}
function text(value: unknown, max = 120): asserts value is string {
  if (typeof value !== "string" || !value.trim() || value.length > max || /[\u0000-\u001f]/.test(value)) reject("Invalid text");
}
function id(value: unknown): asserts value is string { if (typeof value !== "string" || !idPattern.test(value)) reject("Invalid stable ID"); }
export function isCounter(value: unknown): value is string {
  return typeof value === "string" && /^(0|[1-9][0-9]{0,19})$/.test(value) && BigInt(value) <= maxCounter;
}
export function isPublicationHash(value: unknown): value is string { return typeof value === "string" && hashPattern.test(value); }
function counter(value: unknown): void { if (!isCounter(value)) reject("Invalid uint64 decimal counter"); }
function hash(value: unknown): void { if (!isPublicationHash(value)) reject("Invalid publication hash"); }
function integer(value: unknown, min: number, max: number): asserts value is number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < min || value > max) reject("Integer outside limit");
}
function list(value: unknown, min: number, max: number): unknown[] {
  if (!Array.isArray(value) || value.length < min || value.length > max) reject("Array outside limit");
  return value as unknown[];
}
function style(value: unknown): void {
  const item = record(value); keys(item, [], ["foreground", "background", "accent"]);
  for (const color of Object.values(item)) if (typeof color !== "string" || !/^#[a-fA-F0-9]{6}$/.test(color)) reject("Invalid color");
}
function bounded<T>(input: unknown, max: number, check: (value: Record<string, unknown>) => void): Validation<T> {
  try {
    const serialized = JSON.stringify(input);
    if (!serialized || encoder.encode(serialized).length > max) reject("Document exceeds byte budget");
    const value = record(JSON.parse(serialized)); check(value);
    // Detached JSON prevents mutations of an imported object from changing validated data.
    return { ok: true, value: JSON.parse(serialized) as T };
  } catch (error) { return { ok: false, error: error instanceof Error ? error.message : "Invalid document" }; }
}
export function validatePortableUi(input: unknown): Validation<PortableUi> {
  return bounded(input, COMPANION_LIMITS.uiBytes, ui => {
    keys(ui, ["schemaVersion", "id", "title", "controls", "pages", "feedbackWidgets", "assets"]);
    if (ui.schemaVersion !== 1) reject("Unsupported UI version"); id(ui.id); text(ui.title);
    const ids = new Set<string>();
    const unique = (value: unknown) => { id(value); if (ids.has(value)) reject("Duplicate stable ID"); ids.add(value); };
    const controls = list(ui.controls, 1, COMPANION_LIMITS.controls).map(value => {
      const control = record(value);
      const extra = control.kind === "axis" ? ["axisMode", "fallback"] : control.kind === "hat" ? ["fallback"] : [];
      keys(control, ["id", "label", "kind", ...extra], ["style"]); unique(control.id); text(control.label);
      if (control.style !== undefined) style(control.style);
      if (control.kind === "axis") {
        if (control.axisMode !== "bipolar" && control.axisMode !== "unipolar") reject("Invalid axis mode");
        if (typeof control.fallback !== "number" || !Number.isFinite(control.fallback) || control.fallback < (control.axisMode === "bipolar" ? -1 : 0) || control.fallback > 1) reject("Invalid axis fallback");
      } else if (control.kind === "hat") { if (control.fallback !== -1) reject("Invalid neutral hat"); }
      else if (control.kind !== "button") reject("Unsupported control kind");
      return control.id;
    });
    const widgets = list(ui.feedbackWidgets, 0, COMPANION_LIMITS.feedbackWidgets).map(value => {
      const widget = record(value); keys(widget, ["id", "label", "channelId", "kind"], ["style"]);
      unique(widget.id); text(widget.label); id(widget.channelId);
      if (!["text", "indicator", "gauge"].includes(widget.kind as string)) reject("Unsupported feedback kind");
      if (widget.style !== undefined) style(widget.style); return widget.id;
    });
    const placed = new Set<string>();
    for (const value of list(ui.pages, 1, COMPANION_LIMITS.pages)) {
      const page = record(value); keys(page, ["id", "title", "items"]); unique(page.id); text(page.title);
      for (const value of list(page.items, 0, COMPANION_LIMITS.controls + COMPANION_LIMITS.feedbackWidgets)) {
        const item = record(value); keys(item, ["kind", "refId", "bounds"]); id(item.refId);
        if (item.kind !== "control" && item.kind !== "feedback") reject("Invalid page item kind");
        if (!(item.kind === "control" ? controls : widgets).includes(item.refId)) reject("Unknown page item reference");
        if (placed.has(item.refId)) reject("Element appears more than once"); placed.add(item.refId);
        const bounds = record(item.bounds); keys(bounds, ["x", "y", "width", "height"]);
        for (const v of Object.values(bounds)) if (typeof v !== "number" || !Number.isFinite(v) || v < 0 || v > 1) reject("Invalid normalized bounds");
        if (Number(bounds.width) === 0 || Number(bounds.height) === 0 || Number(bounds.x) + Number(bounds.width) > 1 || Number(bounds.y) + Number(bounds.height) > 1) reject("Element outside page");
      }
    }
    if (controls.some(controlId => !placed.has(String(controlId)))) reject("Unplaced control");
    let bytes = 0;
    for (const value of list(ui.assets, 0, 64)) {
      const asset = record(value); keys(asset, ["id", "sha256", "sizeBytes", "mime"]); unique(asset.id);
      if (typeof asset.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(asset.sha256)) reject("Invalid asset digest");
      integer(asset.sizeBytes, 1, COMPANION_LIMITS.assetBytes); bytes += asset.sizeBytes;
      if (!["image/png", "image/jpeg", "image/webp"].includes(asset.mime as string)) reject("Unsupported asset media type");
    }
    if (bytes > COMPANION_LIMITS.totalAssetBytes) reject("Asset total exceeds budget");
  });
}
export function neutralValues(ui: PortableUi): Values {
  return Object.fromEntries(ui.controls.map(control => [control.id, control.kind === "button" ? false : control.fallback]));
}
export function validControlValue(control: Control, value: unknown): value is Value {
  if (control.kind === "button") return typeof value === "boolean";
  if (typeof value !== "number" || !Number.isFinite(value)) return false;
  return control.kind === "hat" ? Number.isInteger(value) && value >= -1 && value <= 7 : value >= (control.axisMode === "bipolar" ? -1 : 0) && value <= 1;
}
function identity(frame: Record<string, unknown>, type: string): void {
  if (frame.version !== 1 || frame.type !== type) reject("Unsupported frame version or type");
  id(frame.clientId); id(frame.sessionId); counter(frame.generation); hash(frame.publicationHash);
}
export function validateHello(input: unknown): Validation<Hello> {
  return bounded(input, COMPANION_LIMITS.frameBytes, frame => {
    keys(frame, ["version", "type", "clientId", "sessionId", "generation", "publicationHash", "hostId", "companionId", "controllerInstanceId"]); identity(frame, "hello");
    id(frame.hostId); id(frame.companionId); id(frame.controllerInstanceId);
  });
}
export function validateInputBatch(input: unknown, ui: PortableUi): Validation<InputBatch> {
  return bounded(input, COMPANION_LIMITS.frameBytes, frame => {
    keys(frame, ["version", "type", "clientId", "sessionId", "generation", "sequence", "publicationHash", "changes"]); identity(frame, "input"); counter(frame.sequence);
    for (const value of list(frame.changes, 1, COMPANION_LIMITS.changesPerBatch)) {
      const change = record(value); keys(change, ["controlId", "value"]);
      const control = ui.controls.find(control => control.id === change.controlId);
      if (!control || !validControlValue(control, change.value)) reject("Unknown control or invalid value");
    }
  });
}
function feedbackValues(input: unknown, ui: PortableUi): void {
  const seen = new Set<string>();
  for (const value of list(input, 0, COMPANION_LIMITS.feedbackWidgets)) {
    const item = record(value); keys(item, ["channelId", "outputId", "status", "provenance", "sourceKind", "sourceAgeMs", "unit"], ["range", "value"]);
    id(item.channelId); id(item.outputId);
    if (!["fresh", "unknown", "stale", "disconnected"].includes(item.status as string)) reject("Invalid feedback status");
    if (!["requested", "local", "host-accepted", "host-submitted", "game-confirmed"].includes(item.provenance as string)) reject("Invalid feedback provenance");
    if (!["simulator", "host", "script"].includes(item.sourceKind as string)) reject("Invalid feedback source");
    if (item.provenance === "game-confirmed" && item.sourceKind !== "simulator") reject("Game confirmation requires a simulator source");
    integer(item.sourceAgeMs, 0, 86400000); text(item.unit, 120);
    if (!ui.feedbackWidgets.some(widget => widget.channelId === item.channelId && widget.id === item.outputId) || seen.has(item.channelId)) reject("Unknown or repeated feedback channel");
    seen.add(item.channelId);
    const hasValue = Object.hasOwn(item, "value");
    if (item.status === "fresh" && !hasValue || ["unknown", "disconnected"].includes(item.status as string) && hasValue) reject("Feedback status/value mismatch");
    if (item.range !== undefined) {
      const range = record(item.range); keys(range, ["min", "max"]);
      if (!ui.feedbackWidgets.filter(widget => widget.channelId === item.channelId).every(widget => widget.kind === "gauge")) reject("Range requires numeric feedback outputs");
      if (typeof range.min !== "number" || typeof range.max !== "number" || !Number.isFinite(range.min) || !Number.isFinite(range.max) || range.min >= range.max) reject("Invalid feedback range");
      if (hasValue && (typeof item.value !== "number" || item.value < range.min || item.value > range.max)) reject("Feedback outside declared range");
    }
    if (hasValue) {
      const widgets = ui.feedbackWidgets.filter(widget => widget.channelId === item.channelId);
      if (widgets.some(widget => widget.kind === "text" ? typeof item.value !== "string" : widget.kind === "indicator" ? typeof item.value !== "boolean" : typeof item.value !== "number")) reject("Feedback widget type mismatch");
      if (typeof item.value === "string") { if (item.value.length > 256 || /[\u0000-\u001f]/.test(item.value)) reject("Invalid feedback text"); }
      else if (typeof item.value !== "boolean" && (typeof item.value !== "number" || !Number.isFinite(item.value))) reject("Invalid feedback value");
    }
  }
}
/** Detached, typed full snapshot. This validates visual data only and never creates input. */
export function validateFeedbackValues(input: unknown, ui: PortableUi): Validation<FeedbackValue[]> {
  const result = bounded<{ values: FeedbackValue[] }>({ values: input }, COMPANION_LIMITS.feedbackBytes, frame => { keys(frame, ["values"]); feedbackValues(frame.values, ui); });
  return result.ok ? { ok: true, value: result.value.values } : result;
}
export function validateFeedback(input: unknown, ui: PortableUi): Validation<FeedbackFrame> {
  return bounded(input, COMPANION_LIMITS.frameBytes, frame => {
    keys(frame, ["version", "type", "clientId", "sessionId", "generation", "sequence", "publicationHash", "values"]); identity(frame, "feedback"); counter(frame.sequence);
    feedbackValues(frame.values, ui);
  });
}
/** C5 logical envelope after bounded native transfer reassembly. State/sequence admission stays native. */
export function validateFeedbackUpdate(input: unknown, ui: PortableUi): Validation<FeedbackUpdateFrame> {
  return bounded(input, COMPANION_LIMITS.feedbackBytes, frame => {
    keys(frame, ["version", "type", "clientId", "sessionId", "generation", "controllerInstanceId", "publicationHash", "sequence", "bindingGeneration", "mode", "values"], ["bindings"]);
    identity(frame, "feedback"); id(frame.controllerInstanceId); counter(frame.sequence); counter(frame.bindingGeneration);
    if (frame.mode !== "snapshot" && frame.mode !== "delta") reject("Invalid feedback update mode");
    feedbackValues(frame.values, ui);
    if (frame.mode === "delta") { if (Object.hasOwn(frame, "bindings")) reject("Delta cannot replace bindings"); return; }
    const seen = new Set<string>();
    const bindings = list(frame.bindings, 0, COMPANION_LIMITS.feedbackWidgets).map(value => {
      const binding = record(value); keys(binding, ["channelId", "outputId"]); id(binding.channelId); id(binding.outputId);
      if (seen.has(binding.channelId) || !ui.feedbackWidgets.some(output => output.channelId === binding.channelId && output.id === binding.outputId)) reject("Invalid feedback snapshot binding");
      seen.add(binding.channelId); return binding;
    });
    for (const value of frame.values as FeedbackValue[]) if (!bindings.some(binding => binding.channelId === value.channelId && binding.outputId === value.outputId)) reject("Unbound feedback snapshot value");
  });
}
export function parseFrame<T>(raw: string, validate: (value: unknown) => Validation<T>): Validation<T> {
  if (typeof raw !== "string" || encoder.encode(raw).length > COMPANION_LIMITS.frameBytes) return { ok: false, error: "Frame exceeds byte budget" };
  try { return validate(JSON.parse(raw)); } catch { return { ok: false, error: "Invalid JSON frame" }; }
}
function canonical(value: unknown): string {
  if (Array.isArray(value)) return "[" + value.map(canonical).join(",") + "]";
  if (value && typeof value === "object") return "{" + Object.keys(value).sort().map(key => JSON.stringify(key) + ":" + canonical((value as Record<string, unknown>)[key])).join(",") + "}";
  return JSON.stringify(value);
}
export async function publicationHash(ui: PortableUi): Promise<string> {
  const result = validatePortableUi(ui); if (!result.ok) throw new Error(result.error);
  const digest = await globalThis.crypto.subtle.digest("SHA-256", encoder.encode(canonical(result.value)));
  return "sha256:" + Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, "0")).join("");
}
function capabilities(ui: PortableUi): unknown {
  return {
    id: ui.id,
    controls: ui.controls.map(control => ({ id: control.id, kind: control.kind, ...(control.kind === "axis" ? { axisMode: control.axisMode, fallback: control.fallback } : {}) })).sort((a, b) => a.id.localeCompare(b.id)),
    feedback: ui.feedbackWidgets.map(({ id, kind, channelId }) => ({ id, kind, channelId })).sort((a, b) => a.id.localeCompare(b.id)),
  };
}
export function comparePublications(previous: PortableUi, next: PortableUi): "identical" | "styling" | "capability" {
  const first = validatePortableUi(previous), second = validatePortableUi(next);
  if (!first.ok) throw new Error(first.error);
  if (!second.ok) throw new Error(second.error);
  if (canonical(first.value) === canonical(second.value)) return "identical";
  return canonical(capabilities(first.value)) === canonical(capabilities(second.value)) ? "styling" : "capability";
}
/** A separate full-state frame; the 64-change incremental batch limit does not apply. */
export function validateBaseline(input: unknown, ui: PortableUi): Validation<BaselineFrame> {
  return bounded(input, COMPANION_LIMITS.frameBytes, frame => {
    keys(frame, ["version", "type", "clientId", "sessionId", "generation", "publicationHash", "sequence", "values"]);
    identity(frame, "baseline");
    if (frame.sequence !== "0") reject("Baseline sequence must be zero");
    const values = record(frame.values), neutral = neutralValues(ui);
    keys(values, Object.keys(neutral));
    if (Object.entries(neutral).some(([id, value]) => values[id] !== value)) reject("Baseline must be fully neutral");
  });
}

/** Quantize normalized wire values into the mapping engine's exact signed/unipolar range. */
export function quantizeAxis(value: number, mode: "bipolar" | "unipolar", fallback: number): number {
  if (mode !== "bipolar" && mode !== "unipolar") throw new Error("Invalid axis mode");
  const min = mode === "bipolar" ? -1 : 0;
  if (!Number.isFinite(fallback) || fallback < min || fallback > 1) throw new Error("Invalid axis fallback");
  const normalized = Math.max(min, Math.min(1, Number.isFinite(value) ? value : fallback));
  const integer = normalized < 0 ? -Math.round(-normalized * 32768) : Math.round(normalized * 32767);
  return integer === 0 ? 0 : integer;
}


export function validateHeartbeat(input: unknown): Validation<HeartbeatFrame> {
  return bounded(input, COMPANION_LIMITS.frameBytes, frame => {
    keys(frame, ["version", "type", "clientId", "sessionId", "generation", "publicationHash", "sequence"]);
    identity(frame, "heartbeat"); counter(frame.sequence);
  });
}
