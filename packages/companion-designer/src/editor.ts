import { validatePortableUi } from "../../companion-contracts/src/index.ts";
import type { Control, FeedbackWidget } from "../../companion-contracts/src/index.ts";
import { DESIGNER_LIMITS } from "./types.ts";
import type { Bounds, DesignerDocument, DesignerHistory, DesignerWidget, LogicalBinding, LogicalTarget, Viewport, WidgetKind } from "./types.ts";
import { assert, checked, clone, newId, normalizeToPortableUi, validateDesignerDocument } from "./model.ts";
export const widgetPresets: readonly { kind: WidgetKind; title: string }[] = [
  { kind: "button", title: "Button" }, { kind: "selector", title: "Selector" }, { kind: "hat", title: "Hat" },
  { kind: "encoder", title: "Encoder" }, { kind: "slider", title: "Slider" }, { kind: "xy", title: "XY pad" },
  { kind: "indicator", title: "Indicator" }, { kind: "text", title: "Text" }, { kind: "numeric", title: "Numeric" },
  { kind: "icon", title: "Icon" }, { kind: "gauge", title: "Gauge" },
];
export function defaultWidgetConfig(kind: WidgetKind): DesignerWidget["config"] {
  switch (kind) {
    case "button": return { buttonMode: "momentary", pulseMs: 0, repeatMs: 0 };
    case "selector": return { options: [{ label: "Off", value: 0 }, { label: "Standby", value: 0.5 }, { label: "On", value: 1 }] };
    case "encoder": return { pulseMs: 50, repeatMs: 0 };
    case "slider": return { centering: "spring", deadzone: 0.05, inverted: false, orientation: "horizontal" };
    case "xy": return { centering: "spring", deadzone: 0.05, invertX: false, invertY: false };
    case "text": return { prefix: "", suffix: "" };
    case "numeric": case "gauge": return { min: 0, max: 100, unit: "%", precision: 0 };
    case "icon": return { symbol: "power" };
    default: return {};
  }
}
function bindAvailable(doc: DesignerDocument, control: Control): void {
  if (!doc.targets.length) doc.targets.push({ id: newId("target"), label: "Logical controller", axes: [], buttons: 0, hats: 0 });
  const target = doc.targets[0];
  const busy = doc.bindings.filter(binding => binding.targetId === target.id && binding.kind === control.kind).map(binding => binding.index);
  let index = -1;
  if (control.kind === "axis") {
    index = target.axes.findIndex((mode, index) => mode === control.axisMode && !busy.includes(index));
    if (index < 0 && target.axes.length < 16) { index = target.axes.length; target.axes.push(control.axisMode); }
  } else {
    const key = control.kind === "button" ? "buttons" : "hats", max = control.kind === "button" ? 128 : 4;
    index = Array.from({ length: target[key] }, (_, i) => i).find(i => !busy.includes(i)) ?? -1;
    if (index < 0 && target[key] < max) { index = target[key]; target[key]++; }
  }
  if (index >= 0) doc.bindings.push({ controlId: control.id, targetId: target.id, kind: control.kind, index });
}
function addWidget(doc: DesignerDocument, kind: WidgetKind, pageId: string, bounds?: Bounds, axisMode?: "unipolar" | "bipolar"): DesignerWidget {
  const title = widgetPresets.find(preset => preset.kind === kind)?.title; assert(title, "Unknown preset");
  const controlKinds = { button: ["button"], selector: ["axis"], hat: ["hat"], encoder: ["button", "button"], slider: ["axis"], xy: ["axis", "axis"] }[kind as "button"] as string[] | undefined;
  const controls = (controlKinds ?? []).map((type, index) => {
    const base = { id: newId("input"), label: title + (index ? " " + (index + 1) : "") };
    const control: Control = type === "axis" ? { ...base, kind: "axis", axisMode: kind === "selector" ? "unipolar" : axisMode ?? "bipolar", fallback: 0 } : type === "hat" ? { ...base, kind: "hat", fallback: -1 } : { ...base, kind: "button" };
    doc.ui.controls.push(control); bindAvailable(doc, control); return control;
  });
  const feedbackKind = { indicator: "indicator", text: "text", numeric: "gauge", icon: "indicator", gauge: "gauge" }[kind as "indicator"];
  const feedback: FeedbackWidget[] = feedbackKind ? [{ id: newId("output"), label: title, kind: feedbackKind as FeedbackWidget["kind"], channelId: newId("channel") }] : [];
  doc.ui.feedbackWidgets.push(...feedback);
  const offset = doc.presentation.widgets.filter(widget => widget.pageId === pageId).length % 4 * 0.05;
  const widget = { id: newId("widget"), pageId, kind, label: title, bounds: bounds ?? { x: 0.05 + offset, y: 0.05 + offset, width: 0.3, height: 0.25 }, controlIds: controls.map(control => control.id), feedbackIds: feedback.map(output => output.id), config: defaultWidgetConfig(kind) } as DesignerWidget;
  doc.presentation.widgets.push(widget); return widget;
}
function removeWidget(doc: DesignerDocument, id: string): void {
  const widget = doc.presentation.widgets.find(widget => widget.id === id); assert(widget, "Unknown widget");
  doc.presentation.widgets = doc.presentation.widgets.filter(widget => widget.id !== id);
  doc.ui.controls = doc.ui.controls.filter(control => !widget.controlIds.includes(control.id));
  doc.ui.feedbackWidgets = doc.ui.feedbackWidgets.filter(output => !widget.feedbackIds.includes(output.id));
  doc.bindings = doc.bindings.filter(binding => !widget.controlIds.includes(binding.controlId));
}
export function newDocument(title = "Untitled panel"): DesignerDocument {
  const pageId = newId("page");
  const doc: DesignerDocument = {
    schemaVersion: 2,
    ui: { schemaVersion: 1, id: newId("ui"), title, controls: [], feedbackWidgets: [], pages: [{ id: pageId, title: "Page 1", items: [] }], assets: [] },
    presentation: { widgets: [], groups: [], viewports: [{ pageId, width: 1280, height: 800 }], grid: { columns: 24, rows: 16, snap: true } },
    targets: [{ id: newId("target"), label: "Logical controller", axes: ["bipolar", "bipolar"], buttons: 8, hats: 1 }], bindings: [],
  };
  addWidget(doc, "button", pageId); doc.ui = normalizeToPortableUi(doc); return checked(doc);
}
export function createDesignerHistory(document: DesignerDocument): DesignerHistory { return { past: [], present: checked(document), future: [] }; }
export type DesignerCommand =
  | { type: "replace"; document: DesignerDocument }
  | { type: "add-widget"; kind: WidgetKind; pageId?: string; bounds?: Bounds; axisMode?: "unipolar" | "bipolar" }
  | { type: "update-widget"; id: string; patch: Record<string, unknown> }
  | { type: "move-widget"; id: string; bounds: Bounds }
  | { type: "delete-widget" | "duplicate-widget"; id: string }
  | { type: "add-page"; title: string }
  | { type: "delete-page"; id: string }
  | { type: "add-group"; pageId: string; label: string; widgetIds: string[] }
  | { type: "ungroup"; id: string }
  | { type: "align"; ids: string[]; edge: "left" | "right" | "top" | "bottom" | "center-x" | "center-y" }
  | { type: "translate"; ids: string[]; dx: number; dy: number }
  | { type: "snap"; ids: string[] }
  | { type: "set-grid"; columns: number; rows: number; snap: boolean }
  | ({ type: "set-viewport" } & Viewport)
  | { type: "add-target"; target?: LogicalTarget }
  | { type: "update-target"; id: string; patch: Partial<Omit<LogicalTarget, "id">> }
  | { type: "remove-target"; id: string }
  | ({ type: "bind" } & LogicalBinding)
  | { type: "unbind"; controlId: string };
export function applyDesignerCommand(history: DesignerHistory, command: DesignerCommand): DesignerHistory {
  let doc = clone(history.present);
  const selected = (ids: string[]) => { const widgets = ids.map(id => doc.presentation.widgets.find(widget => widget.id === id)); assert(ids.length > 0 && widgets.every(Boolean) && new Set(ids).size === ids.length, "Invalid selection"); const result = widgets as DesignerWidget[]; assert(new Set(result.map(widget => widget.pageId)).size === 1, "Selection spans pages"); return result; };
  switch (command.type) {
    case "replace": doc = clone(command.document); break;
    case "add-widget": addWidget(doc, command.kind, command.pageId ?? doc.ui.pages[0].id, command.bounds, command.axisMode); break;
    case "update-widget": {
      assert(Object.keys(command.patch).every(key => ["label", "bounds", "config", "style", "groupId", "pageId"].includes(key)), "Stable binding IDs cannot be edited");
      const widget = selected([command.id])[0]; Object.assign(widget, clone(command.patch)); break;
    }
    case "move-widget": selected([command.id])[0].bounds = clone(command.bounds); break;
    case "delete-widget": removeWidget(doc, command.id); break;
    case "duplicate-widget": {
      const old = selected([command.id])[0], copy = clone(old); copy.id = newId("widget"); copy.label += " copy";
      copy.bounds.x = Math.min(1 - copy.bounds.width, copy.bounds.x + 0.025); copy.bounds.y = Math.min(1 - copy.bounds.height, copy.bounds.y + 0.025);
      copy.controlIds = old.controlIds.map(id => { const control = clone(doc.ui.controls.find(control => control.id === id)!); control.id = newId("input"); doc.ui.controls.push(control); bindAvailable(doc, control); return control.id; });
      copy.feedbackIds = old.feedbackIds.map(id => { const output = clone(doc.ui.feedbackWidgets.find(output => output.id === id)!); output.id = newId("output"); output.channelId = newId("channel"); doc.ui.feedbackWidgets.push(output); return output.id; });
      doc.presentation.widgets.push(copy); break;
    }
    case "add-page": {
      const id = newId("page"); doc.ui.pages.push({ id, title: command.title, items: [] }); doc.presentation.viewports.push({ pageId: id, width: 1280, height: 800 }); break;
    }
    case "delete-page":
      assert(doc.ui.pages.length > 1, "A panel needs a page"); assert(doc.ui.pages.some(page => page.id === command.id), "Unknown page");
      for (const widget of [...doc.presentation.widgets].filter(widget => widget.pageId === command.id)) removeWidget(doc, widget.id);
      doc.ui.pages = doc.ui.pages.filter(page => page.id !== command.id); doc.presentation.groups = doc.presentation.groups.filter(group => group.pageId !== command.id); doc.presentation.viewports = doc.presentation.viewports.filter(viewport => viewport.pageId !== command.id); break;
    case "add-group": {
      const widgets = selected(command.widgetIds); assert(widgets.every(widget => widget.pageId === command.pageId), "Group page mismatch");
      const group = { id: newId("group"), pageId: command.pageId, label: command.label }; doc.presentation.groups.push(group); for (const widget of widgets) widget.groupId = group.id; break;
    }
    case "ungroup":
      assert(doc.presentation.groups.some(group => group.id === command.id), "Unknown group"); doc.presentation.groups = doc.presentation.groups.filter(group => group.id !== command.id); for (const widget of doc.presentation.widgets) if (widget.groupId === command.id) delete widget.groupId; break;
    case "align": {
      const widgets = selected(command.ids), first = widgets[0].bounds;
      for (const widget of widgets) {
        const b = widget.bounds;
        if (command.edge === "left") b.x = first.x;
        else if (command.edge === "right") b.x = first.x + first.width - b.width;
        else if (command.edge === "top") b.y = first.y;
        else if (command.edge === "bottom") b.y = first.y + first.height - b.height;
        else if (command.edge === "center-x") b.x = first.x + (first.width - b.width) / 2;
        else b.y = first.y + (first.height - b.height) / 2;
      } break;
    }
    case "translate": for (const widget of selected(command.ids)) { widget.bounds.x += command.dx; widget.bounds.y += command.dy; } break;
    case "snap": {
      const { columns, rows } = doc.presentation.grid;
      for (const widget of selected(command.ids)) { widget.bounds.x = Math.min(1 - widget.bounds.width, Math.round(widget.bounds.x * columns) / columns); widget.bounds.y = Math.min(1 - widget.bounds.height, Math.round(widget.bounds.y * rows) / rows); } break;
    }
    case "set-grid": doc.presentation.grid = { columns: command.columns, rows: command.rows, snap: command.snap }; break;
    case "set-viewport": {
      const { type: _, ...viewport } = command; const index = doc.presentation.viewports.findIndex(viewport => viewport.pageId === command.pageId); assert(index >= 0, "Unknown viewport"); doc.presentation.viewports[index] = clone(viewport); break;
    }
    case "add-target": doc.targets.push(command.target ?? { id: newId("target"), label: "Logical controller " + (doc.targets.length + 1), axes: ["bipolar", "bipolar"], buttons: 8, hats: 1 }); break;
    case "update-target": {
      const target = doc.targets.find(target => target.id === command.id); assert(target, "Unknown target"); Object.assign(target, clone(command.patch)); break;
    }
    case "remove-target": assert(doc.targets.some(target => target.id === command.id), "Unknown target"); doc.targets = doc.targets.filter(target => target.id !== command.id); doc.bindings = doc.bindings.filter(binding => binding.targetId !== command.id); break;
    case "bind": {
      const { type: _, ...binding } = command; doc.bindings = doc.bindings.filter(binding => binding.controlId !== command.controlId); doc.bindings.push(binding); break;
    }
    case "unbind": doc.bindings = doc.bindings.filter(binding => binding.controlId !== command.controlId); break;
  }
  doc.ui = normalizeToPortableUi(doc); doc = checked(doc);
  return { past: [...history.past, clone(history.present)].slice(-DESIGNER_LIMITS.history), present: doc, future: [] };
}
export function undoDesigner(history: DesignerHistory): DesignerHistory {
  if (!history.past.length) return clone(history);
  return { past: clone(history.past.slice(0, -1)), present: clone(history.past.at(-1)!), future: [clone(history.present), ...clone(history.future)].slice(0, DESIGNER_LIMITS.history) };
}
export function redoDesigner(history: DesignerHistory): DesignerHistory {
  if (!history.future.length) return clone(history);
  return { past: [...clone(history.past), clone(history.present)].slice(-DESIGNER_LIMITS.history), present: clone(history.future[0]), future: clone(history.future.slice(1)) };
}
export function migratePortableUi(input: unknown): DesignerDocument {
  const validation = validatePortableUi(input); assert(validation.ok, validation.ok ? "" : validation.error);
  const logical = validation.value;
  const doc: DesignerDocument = { schemaVersion: 2, ui: clone(logical), targets: [], bindings: [], presentation: { widgets: [], groups: [], viewports: [], grid: { columns: 24, rows: 16, snap: true } } };
  // Validate legacy JSON before dereferencing it or producing partial migrated state.

  for (const page of doc.ui.pages ?? []) doc.presentation.viewports.push({ pageId: page.id, width: 1280, height: 800 });
  for (const control of doc.ui.controls ?? []) {
    const page = doc.ui.pages.find(page => page.items.some(item => item.kind === "control" && item.refId === control.id)); assert(page, "Legacy control has no page");
    const item = page.items.find(item => item.refId === control.id)!;
    const kind: WidgetKind = control.kind === "axis" ? "slider" : control.kind;
    doc.presentation.widgets.push({ id: newId("widget"), pageId: page.id, label: control.label, bounds: clone(item.bounds), kind, controlIds: [control.id], feedbackIds: [], config: defaultWidgetConfig(kind), ...(control.style ? { style: clone(control.style) } : {}) } as DesignerWidget);
    bindAvailable(doc, control);
  }
  for (const output of doc.ui.feedbackWidgets ?? []) {
    const page = doc.ui.pages.find(page => page.items.some(item => item.kind === "feedback" && item.refId === output.id)) ?? doc.ui.pages[0];
    const item = page.items.find(item => item.refId === output.id);
    doc.presentation.widgets.push({ id: newId("widget"), pageId: page.id, label: output.label, bounds: item?.bounds ?? { x: 0.1, y: 0.1, width: 0.3, height: 0.2 }, kind: output.kind, controlIds: [], feedbackIds: [output.id], config: defaultWidgetConfig(output.kind), ...(output.style ? { style: clone(output.style) } : {}) } as DesignerWidget);
  }
  doc.ui = normalizeToPortableUi(doc); return checked(doc);
}
