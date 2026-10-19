import { validatePortableUi } from "../../companion-contracts/src/index.ts";
import type { Control, FeedbackWidget, Validation } from "../../companion-contracts/src/index.ts";
import { DESIGNER_LIMITS } from "./types.ts";
import type { Bounds, DesignerDocument, DesignerWidget, LogicalTarget, ViewportRegion } from "./types.ts";
export const clone = <T>(value: T): T => structuredClone(value);
export function assert(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
export function object(value: unknown): Record<string, unknown> {
  assert(value && typeof value === "object" && !Array.isArray(value), "Expected object");
  return value as Record<string, unknown>;
}
export function exact(value: unknown, required: string[], optional: string[] = []): Record<string, unknown> {
  const item = object(value);
  assert(required.every(key => Object.hasOwn(item, key)) && Object.keys(item).every(key => required.includes(key) || optional.includes(key)), "Unexpected or missing fields");
  return item;
}
export function stableId(value: unknown): asserts value is string { assert(typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value), "Invalid stable ID"); }
export function label(value: unknown, allowEmpty = false): asserts value is string { assert(typeof value === "string" && value.length <= 120 && (allowEmpty || value.trim()) && !/[\u0000-\u001f]/.test(value), "Invalid label"); }
export function number(value: unknown, min: number, max: number): asserts value is number { assert(typeof value === "number" && Number.isFinite(value) && value >= min && value <= max, "Number outside bounds"); }
export function integer(value: unknown, min: number, max: number): asserts value is number { number(value, min, max); assert(Number.isInteger(value), "Expected integer"); }
function array(value: unknown, min: number, max: number): unknown[] { assert(Array.isArray(value) && value.length >= min && value.length <= max, "Array outside bounds"); return value; }
function colorStyle(value: unknown): void {
  const style = exact(value, [], ["foreground", "background", "accent"]);
  for (const color of Object.values(style)) assert(typeof color === "string" && /^#[a-fA-F0-9]{6}$/.test(color), "Invalid color");
}
function bounds(value: unknown): void {
  const b = exact(value, ["x", "y", "width", "height"]);
  for (const v of Object.values(b)) number(v, 0, 1);
  assert(Number(b.width) > 0 && Number(b.height) > 0 && Number(b.x) + Number(b.width) <= 1 && Number(b.y) + Number(b.height) <= 1, "Bounds outside page");
}
const geometryTolerance = 1e-9;
export function boundsContain(container: Bounds, child: Bounds): boolean {
  return child.x >= container.x - geometryTolerance && child.y >= container.y - geometryTolerance
    && child.x + child.width <= container.x + container.width + geometryTolerance
    && child.y + child.height <= container.y + container.height + geometryTolerance;
}
export function boundsOverlap(a: Bounds, b: Bounds): boolean {
  return Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x) > geometryTolerance
    && Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y) > geometryTolerance;
}
/** Regions share widget coordinates: normalized within the screen's optional safe insets. */
export function viewportRegions(document: DesignerDocument, pageId: string): ViewportRegion[] {
  const viewport = document.presentation.viewports.find(viewport => viewport.pageId === pageId);
  assert(viewport, "Unknown companion screen page");
  if (viewport.regions !== undefined) return clone(viewport.regions);
  const usedIds = new Set([
    document.ui.id,
    ...document.ui.pages.map(page => page.id), ...document.ui.controls.map(control => control.id),
    ...document.ui.feedbackWidgets.map(widget => widget.id), ...document.ui.assets.map(asset => asset.id),
    ...document.presentation.widgets.map(widget => widget.id), ...document.presentation.groups.map(group => group.id),
    ...document.targets.map(target => target.id),
    ...document.presentation.viewports.flatMap(viewport => viewport.regions?.map(region => region.id) ?? []),
  ]);
  const base = "viewport-" + pageId.slice(0, 55);
  let id = base;
  for (let suffix = 2; usedIds.has(id); suffix++) {
    const ending = "-" + suffix; id = base.slice(0, 64 - ending.length) + ending;
  }
  return [{ id, label: "Viewport 1", bounds: { x: 0, y: 0, width: 1, height: 1 } }];
}
export function widgetViewport(document: DesignerDocument, widget: Pick<DesignerWidget, "pageId" | "bounds">): ViewportRegion | undefined {
  return viewportRegions(document, widget.pageId).find(region => boundsContain(region.bounds, widget.bounds));
}
const inputKinds: Record<string, string[]> = { button: ["button"], selector: ["axis"], hat: ["hat"], encoder: ["button", "button"], slider: ["axis"], xy: ["axis", "axis"] };
const outputKinds: Record<string, string> = { indicator: "indicator", text: "text", numeric: "gauge", icon: "indicator", gauge: "gauge" };
function validateConfig(widget: DesignerWidget, controls: Control[], assetIds: string[]): void {
  const c = object(widget.config);
  switch (widget.kind) {
    case "button":
      exact(c, ["buttonMode", "pulseMs", "repeatMs"], ["iconAssetId"]); if (c.iconAssetId !== undefined) assert(assetIds.includes(String(c.iconAssetId)), "Missing button icon asset"); assert(["momentary", "toggle", "pulse", "repeat"].includes(String(c.buttonMode)), "Invalid button mode");
      integer(c.pulseMs, 0, 1000); integer(c.repeatMs, 0, 5000);
      if (c.buttonMode === "momentary" || c.buttonMode === "toggle") assert(c.pulseMs === 0 && c.repeatMs === 0, "Momentary/toggle cannot repeat");
      else { assert(Number(c.pulseMs) > 0, "Pulse duration required"); if (c.buttonMode === "pulse") assert(c.repeatMs === 0, "Pulse cannot repeat"); else assert(Number(c.repeatMs) >= 20 && Number(c.pulseMs) < Number(c.repeatMs), "Invalid repeat duration"); }
      break;
    case "selector": {
      exact(c, ["options"]); const values: number[] = [], axis = controls[0];
      assert(axis?.kind === "axis", "Selector requires axis");
      for (const value of array(c.options, 2, 16)) { const option = exact(value, ["label", "value"]); label(option.label); number(option.value, axis.axisMode === "unipolar" ? 0 : -1, 1); assert(!values.includes(option.value), "Duplicate selector value"); values.push(option.value); }
      assert(values.includes(axis.fallback), "Selector fallback must name an option"); break;
    }
    case "image": exact(c, [], ["assetId"]); if(c.assetId !== undefined) assert(assetIds.includes(String(c.assetId)), "Missing image asset"); break;
    case "hat": case "indicator": exact(c, []); break;
    case "encoder":
      exact(c, ["pulseMs", "repeatMs"]); integer(c.pulseMs, 1, 1000); integer(c.repeatMs, 0, 5000);
      assert(c.repeatMs === 0 || Number(c.repeatMs) >= 20 && Number(c.pulseMs) < Number(c.repeatMs), "Invalid encoder repeat"); break;
    case "slider": case "xy":
      exact(c, widget.kind === "slider" ? ["centering", "deadzone", "inverted", "orientation"] : ["centering", "deadzone", "invertX", "invertY"]);
      assert(c.centering === "spring" || c.centering === "latched", "Invalid centering"); number(c.deadzone, 0, 0.49);
      if (widget.kind === "slider") assert(typeof c.inverted === "boolean" && (c.orientation === "horizontal" || c.orientation === "vertical"), "Invalid slider properties");
      else assert(typeof c.invertX === "boolean" && typeof c.invertY === "boolean", "Invalid XY properties"); break;
    case "text": exact(c, ["prefix", "suffix"]); label(c.prefix, true); label(c.suffix, true); break;
    case "numeric": case "gauge":
      exact(c, ["min", "max", "unit", "precision"]); number(c.min, -1e9, 1e9); number(c.max, -1e9, 1e9); assert(Number(c.min) < Number(c.max), "Invalid feedback range"); label(c.unit, true); integer(c.precision, 0, 4); break;
    case "icon":
      exact(c, ["symbol"], ["iconAssetId"]); assert(["power", "warning", "gear", "check"].includes(String(c.symbol)), "Invalid icon symbol");
      if (c.iconAssetId !== undefined) assert(assetIds.includes(String(c.iconAssetId)), "Missing icon asset"); break;
  }
}
export function validateDesignerDocument(input: unknown): Validation<DesignerDocument> {
  try {
    const json = JSON.stringify(input); assert(json && new TextEncoder().encode(json).length <= 1024 * 1024, "Designer document exceeds 1 MiB");
    const doc = exact(JSON.parse(json), ["schemaVersion", "ui", "presentation", "targets", "bindings"]);
    assert(doc.schemaVersion === 2, "Unsupported designer schema");
    const logical = validatePortableUi(doc.ui, { allowEmptyControls:true }); assert(logical.ok, logical.ok ? "" : logical.error);
    const ui = logical.value, p = exact(doc.presentation, ["widgets", "groups", "viewports", "grid"]);
    const usedIds = new Set([...ui.controls.map(c => c.id), ...ui.feedbackWidgets.map(w => w.id), ...ui.pages.map(p => p.id), ...ui.assets.map(a => a.id)]);
    const newId = (value: unknown) => { stableId(value); assert(!usedIds.has(value), "Duplicate stable ID"); usedIds.add(value); };
    const pageIds = ui.pages.map(page => page.id);
    const groups = array(p.groups, 0, DESIGNER_LIMITS.groups).map(value => {
      const group = exact(value, ["id", "pageId", "label"]); newId(group.id); label(group.label); assert(pageIds.includes(String(group.pageId)), "Unknown group page"); return group;
    });
    const grid = exact(p.grid, ["columns", "rows", "snap"]); integer(grid.columns, 1, 128); integer(grid.rows, 1, 128); assert(typeof grid.snap === "boolean", "Invalid snap");
    const viewportPages: string[] = [];
    for (const value of array(p.viewports, ui.pages.length, ui.pages.length)) {
      const viewport = exact(value, ["pageId", "width", "height"], ["safeInsets", "backgroundAssetId", "regions"]);
      assert(pageIds.includes(String(viewport.pageId)) && !viewportPages.includes(String(viewport.pageId)), "Invalid viewport page"); viewportPages.push(String(viewport.pageId));
      if (viewport.backgroundAssetId !== undefined) assert(ui.assets.some(asset => asset.id === viewport.backgroundAssetId), "Missing page background asset");
      integer(viewport.width, 320, 8192); integer(viewport.height, 320, 8192);
      if (viewport.regions !== undefined) {
        assert(Array.isArray(viewport.regions) && viewport.regions.length > 0 && viewport.regions.length <= DESIGNER_LIMITS.viewportRegions, "A companion screen needs between 1 and 32 viewports");
        const regions: ViewportRegion[] = [];
        for (const value of viewport.regions) {
          const region = object(value);
          assert(["id", "label", "bounds"].every(key => Object.hasOwn(region, key)), "A viewport needs an ID, label and bounds");
          exact(region, ["id", "label", "bounds"]); newId(region.id); label(region.label);
          try { bounds(region.bounds); } catch { throw new Error(`Viewport "${region.label}" must stay inside the companion screen with a positive width and height`); }
          const typedRegion = region as unknown as ViewportRegion;
          assert(!regions.some(other => boundsOverlap(other.bounds, typedRegion.bounds)), `Viewport "${region.label}" overlaps another viewport. Move or resize it so the viewports do not overlap`);
          regions.push(typedRegion);
        }
      }
      if (viewport.safeInsets !== undefined) {
        const insets = exact(viewport.safeInsets, ["top", "right", "bottom", "left"]);
        for (const value of Object.values(insets)) number(value, 0, 0.25);
        assert(Number(insets.left) + Number(insets.right) < 0.5 && Number(insets.top) + Number(insets.bottom) < 0.5, "Invalid safe insets");
      }
    }
    const controlIds: string[] = [], feedbackIds: string[] = [];
    const widgets = array(p.widgets, 0, DESIGNER_LIMITS.widgets) as DesignerWidget[];
    for (const widget of widgets) {
      exact(widget, ["id", "pageId", "kind", "label", "bounds", "controlIds", "feedbackIds", "config"], ["groupId", "style"]);
      newId(widget.id); label(widget.label); bounds(widget.bounds); assert(pageIds.includes(widget.pageId), "Unknown widget page");
      const regions = viewportRegions(doc as unknown as DesignerDocument, widget.pageId);
      assert(regions.some(region => boundsContain(region.bounds, widget.bounds)), `Widget "${widget.label}" must fit entirely inside one viewport`);
      if (widget.style !== undefined) colorStyle(widget.style);
      if (widget.groupId !== undefined) assert(groups.some(group => group.id === widget.groupId && group.pageId === widget.pageId), "Invalid group reference");
      const expected = inputKinds[widget.kind] ?? [], output = outputKinds[widget.kind];
      assert(expected.length > 0 || output || widget.kind === "image", "Unsupported widget");
      assert(array(widget.controlIds, expected.length, expected.length).length === expected.length, "Invalid input arity");
      assert(array(widget.feedbackIds, output ? 1 : 0, output ? 1 : 0).length === (output ? 1 : 0), "Invalid feedback arity");
      const controls = widget.controlIds.map((id, index) => { const control = ui.controls.find(control => control.id === id); assert(control?.kind === expected[index] && !controlIds.includes(id), "Incompatible or duplicate input binding"); controlIds.push(id); return control; });
      for (const id of widget.feedbackIds) { const feedback = ui.feedbackWidgets.find(widget => widget.id === id); assert(feedback?.kind === output && !feedbackIds.includes(id), "Incompatible feedback binding"); feedbackIds.push(id); }
      validateConfig(widget, controls, ui.assets.map(asset => asset.id));
    }
    assert(controlIds.length === ui.controls.length && feedbackIds.length === ui.feedbackWidgets.length, "Unbound primitive");
    const targets = array(doc.targets, 0, DESIGNER_LIMITS.targets) as LogicalTarget[];
    for (const target of targets) {
      exact(target, ["id", "label", "axes", "buttons", "hats"]); newId(target.id); label(target.label);
      for (const axis of array(target.axes, 0, 16)) assert(axis === "unipolar" || axis === "bipolar", "Invalid target axis");
      integer(target.buttons, 0, 128); integer(target.hats, 0, 4); assert(target.axes.length + target.buttons + target.hats > 0, "Empty target");
    }
    const bound = new Set<string>(), slots = new Set<string>();
    for (const value of array(doc.bindings, 0, ui.controls.length)) {
      const binding = exact(value, ["controlId", "targetId", "kind", "index"]), control = ui.controls.find(c => c.id === binding.controlId), target = targets.find(t => t.id === binding.targetId);
      assert(control && target && binding.kind === control.kind && !bound.has(String(binding.controlId)), "Invalid logical binding");
      integer(binding.index, 0, binding.kind === "axis" ? target.axes.length - 1 : binding.kind === "button" ? target.buttons - 1 : target.hats - 1);
      if (control.kind === "axis") assert(target.axes[binding.index] === control.axisMode, "Target axis range mismatch");
      const key = target.id + ":" + binding.kind + ":" + binding.index; assert(!slots.has(key), "Target slot already bound"); slots.add(key); bound.add(String(binding.controlId));
    }
    const result = doc as unknown as DesignerDocument;
    assert(canonical(normalizeToPortableUi(result).pages) === canonical(ui.pages), "Presentation and logical page items differ");
    return { ok: true, value: result };
  } catch (error) { return { ok: false, error: error instanceof Error ? error.message : "Invalid designer document" }; }
}
export function normalizeToPortableUi(document: DesignerDocument) {
  const ui = clone(document.ui);
  ui.pages = ui.pages.map(page => ({ ...page, items: document.presentation.widgets.filter(widget => widget.pageId === page.id).flatMap(widget => [
    ...widget.controlIds.map(refId => ({ kind: "control" as const, refId, bounds: { ...widget.bounds } })),
    ...widget.feedbackIds.map(refId => ({ kind: "feedback" as const, refId, bounds: { ...widget.bounds } })),
  ]) }));
  return ui;
}
export function checked(document: DesignerDocument): DesignerDocument {
  const result = validateDesignerDocument(document); assert(result.ok, result.ok ? "" : result.error); return result.value;
}
export function canonical(value: unknown): string {
  if (Array.isArray(value)) return "[" + value.map(canonical).join(",") + "]";
  if (value && typeof value === "object") return "{" + Object.keys(value).sort().map(key => JSON.stringify(key) + ":" + canonical((value as Record<string, unknown>)[key])).join(",") + "}";
  return JSON.stringify(value);
}
export async function digest(bytes: Uint8Array): Promise<string> {
  const copy = new Uint8Array(bytes);
  const result = await crypto.subtle.digest("SHA-256", copy);
  return Array.from(new Uint8Array(result), value => value.toString(16).padStart(2, "0")).join("");
}
export function newId(prefix: string): string { return prefix + "-" + crypto.randomUUID(); }
