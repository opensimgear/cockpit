import { validatePortableUi } from "../../companion-contracts/src/index.ts";
import type { Control, FeedbackWidget } from "../../companion-contracts/src/index.ts";
import { DESIGNER_LIMITS } from "./types.ts";
import type { Bounds, DesignerDocument, DesignerHistory, DesignerWidget, LogicalBinding, LogicalTarget, Viewport, ViewportRegion, WidgetKind } from "./types.ts";
import { assert, boundsContain, boundsOverlap, checked, clone, newId, normalizeToPortableUi, viewportRegions, widgetViewport } from "./model.ts";
export const widgetPresets: readonly { kind: WidgetKind; title: string }[] = [
  { kind: "button", title: "Button" }, { kind: "selector", title: "Selector" }, { kind: "hat", title: "Hat" },
  { kind: "encoder", title: "Encoder" }, { kind: "slider", title: "Slider" }, { kind: "xy", title: "XY pad" },
  { kind: "indicator", title: "Indicator" }, { kind: "text", title: "Text" }, { kind: "numeric", title: "Numeric" },
  { kind: "image", title: "Image" }, { kind: "icon", title: "Icon" }, { kind: "gauge", title: "Gauge" },
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
function addWidget(doc: DesignerDocument, kind: WidgetKind, pageId: string, bounds?: Bounds, axisMode?: "unipolar" | "bipolar", viewportId?: string): DesignerWidget {
  const title = widgetPresets.find(preset => preset.kind === kind)?.title; assert(title, "Unknown preset");
  const regions = viewportRegions(doc, pageId), region = viewportId ? regions.find(region => region.id === viewportId) : regions[0];
  assert(region, "Unknown viewport for the new widget");
  if (bounds && viewportId) assert(boundsContain(region.bounds, bounds), "The widget must fit entirely inside the selected viewport");
  const controlKinds = { button: ["button"], selector: ["axis"], hat: ["hat"], encoder: ["button", "button"], slider: ["axis"], xy: ["axis", "axis"] }[kind as "button"] as string[] | undefined;
  const controls = (controlKinds ?? []).map((type, index) => {
    const base = { id: newId("input"), label: title + (index ? " " + (index + 1) : "") };
    const control: Control = type === "axis" ? { ...base, kind: "axis", axisMode: kind === "selector" ? "unipolar" : axisMode ?? "bipolar", fallback: 0 } : type === "hat" ? { ...base, kind: "hat", fallback: -1 } : { ...base, kind: "button" };
    doc.ui.controls.push(control); bindAvailable(doc, control); return control;
  });
  const feedbackKind = { indicator: "indicator", text: "text", numeric: "gauge", icon: "indicator", gauge: "gauge" }[kind as "indicator"];
  const feedback: FeedbackWidget[] = feedbackKind ? [{ id: newId("output"), label: title, kind: feedbackKind as FeedbackWidget["kind"], channelId: newId("channel") }] : [];
  doc.ui.feedbackWidgets.push(...feedback);
  const offset = doc.presentation.widgets.filter(widget => widget.pageId === pageId && boundsContain(region.bounds, widget.bounds)).length % 4 * 0.05;
  const widget = { id: newId("widget"), pageId, kind, label: title, bounds: bounds ?? { x: region.bounds.x + (0.05 + offset) * region.bounds.width, y: region.bounds.y + (0.05 + offset) * region.bounds.height, width: 0.3 * region.bounds.width, height: 0.25 * region.bounds.height }, controlIds: controls.map(control => control.id), feedbackIds: feedback.map(output => output.id), config: defaultWidgetConfig(kind) } as DesignerWidget;
  doc.presentation.widgets.push(widget); return widget;
}
function screenFor(doc: DesignerDocument, pageId: string): Viewport {
  const screen = doc.presentation.viewports.find(viewport => viewport.pageId === pageId); assert(screen, "Unknown companion screen page"); return screen;
}
function assertViewportBounds(bounds: Bounds): void {
  assert(bounds && [bounds.x, bounds.y, bounds.width, bounds.height].every(value => Number.isFinite(value) && value >= 0 && value <= 1)
    && bounds.width > 0 && bounds.height > 0 && bounds.x + bounds.width <= 1 && bounds.y + bounds.height <= 1,
  "A viewport must stay inside the companion screen with a positive width and height");
}
function transformViewportWidgets(doc: DesignerDocument, pageId: string, from: Bounds, to: Bounds): void {
  for (const widget of doc.presentation.widgets.filter(widget => widget.pageId === pageId && boundsContain(from, widget.bounds))) {
    const old = widget.bounds;
    const x = to.x + (old.x - from.x) / from.width * to.width, y = to.y + (old.y - from.y) / from.height * to.height;
    widget.bounds = { x, y, width: Math.min(old.width / from.width * to.width, to.x + to.width - x, 1 - x), height: Math.min(old.height / from.height * to.height, to.y + to.height - y, 1 - y) };
  }
}
/** Find the largest empty rectangle using viewport x boundaries and gaps in their y coverage. */
function freeViewportBounds(regions: ViewportRegion[]): Bounds | undefined {
  const edges = [...new Set([0, 1, ...regions.flatMap(region => [region.bounds.x, region.bounds.x + region.bounds.width])])].sort((a, b) => a - b);
  let result: Bounds | undefined;
  const consider = (x: number, right: number, y: number, bottom: number) => {
    const candidate = { x, y, width: right - x, height: bottom - y };
    if (candidate.width > 1e-9 && candidate.height > 1e-9 && (!result || candidate.width * candidate.height > result.width * result.height)) result = candidate;
  };
  for (let left = 0; left < edges.length - 1; left++) for (let right = left + 1; right < edges.length; right++) {
    const x = edges[left], end = edges[right];
    const coverage = regions.filter(region => region.bounds.x < end - 1e-9 && region.bounds.x + region.bounds.width > x + 1e-9).map(region => region.bounds).sort((a, b) => a.y - b.y);
    let bottom = 0;
    for (const bounds of coverage) { consider(x, end, bottom, bounds.y); bottom = Math.max(bottom, bounds.y + bounds.height); }
    consider(x, end, bottom, 1);
  }
  return result;
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
    ui: { schemaVersion: 1, id: newId("ui"), title, controls: [], feedbackWidgets: [], pages: [{ id: pageId, title: "Page 0", items: [] }], assets: [] },
    presentation: { widgets: [], groups: [], viewports: [{ pageId, width: 1280, height: 800 }], grid: { columns: 24, rows: 16, snap: true } },
    targets: [{ id: newId("target"), label: "Logical controller", axes: ["bipolar", "bipolar"], buttons: 8, hats: 1 }], bindings: [],
  };
  addWidget(doc, "button", pageId); doc.ui = normalizeToPortableUi(doc); return checked(doc);
}
export function createDesignerHistory(document: DesignerDocument): DesignerHistory { return { past: [], present: migrateBackgroundImages(document), future: [] }; }
export type DesignerCommand =
  | { type: "replace"; document: DesignerDocument }
  | { type: "add-widget"; kind: WidgetKind; pageId?: string; viewportId?: string; bounds?: Bounds; axisMode?: "unipolar" | "bipolar" }
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
  | { type: "add-viewport"; pageId: string; sourceId?: string }
  | { type: "update-viewport"; pageId: string; id: string; patch: Partial<ViewportRegion> }
  | { type: "remove-viewport"; pageId: string; id: string }
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
    case "add-widget": addWidget(doc, command.kind, command.pageId ?? doc.ui.pages[0].id, command.bounds, command.axisMode, command.viewportId); break;
    case "update-widget": {
      assert(Object.keys(command.patch).every(key => ["label", "bounds", "config", "style", "groupId", "pageId"].includes(key)), "Stable binding IDs cannot be edited");
      const widget = selected([command.id])[0]; Object.assign(widget, clone(command.patch)); break;
    }
    case "move-widget": selected([command.id])[0].bounds = clone(command.bounds); break;
    case "delete-widget": removeWidget(doc, command.id); break;
    case "duplicate-widget": {
      const old = selected([command.id])[0], copy = clone(old); copy.id = newId("widget"); copy.label += " copy";
      const region = widgetViewport(doc, old)!;
      copy.bounds.x = Math.min(region.bounds.x + region.bounds.width - copy.bounds.width, copy.bounds.x + 0.025); copy.bounds.y = Math.min(region.bounds.y + region.bounds.height - copy.bounds.height, copy.bounds.y + 0.025);
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
      doc.ui.pages = doc.ui.pages.filter(page => page.id !== command.id);
      doc.ui.pages.forEach((page, index) => { if (/^Page \d+$/.test(page.title)) page.title = `Page ${index}`; });
      doc.presentation.groups = doc.presentation.groups.filter(group => group.pageId !== command.id); doc.presentation.viewports = doc.presentation.viewports.filter(viewport => viewport.pageId !== command.id); break;
    case "add-group": {
      const widgets = selected(command.widgetIds); assert(widgets.every(widget => widget.pageId === command.pageId), "Group page mismatch");
      const group = { id: newId("group"), pageId: command.pageId, label: command.label }; doc.presentation.groups.push(group); for (const widget of widgets) widget.groupId = group.id; break;
    }
    case "ungroup":
      assert(doc.presentation.groups.some(group => group.id === command.id), "Unknown group"); doc.presentation.groups = doc.presentation.groups.filter(group => group.id !== command.id); for (const widget of doc.presentation.widgets) if (widget.groupId === command.id) delete widget.groupId; break;
    case "align": {
      const widgets = selected(command.ids), first = widgets[0].bounds;
      const left = Math.min(...widgets.map(widget => widget.bounds.x));
      const right = Math.max(...widgets.map(widget => widget.bounds.x + widget.bounds.width));
      const top = Math.min(...widgets.map(widget => widget.bounds.y));
      const bottom = Math.max(...widgets.map(widget => widget.bounds.y + widget.bounds.height));
      for (const widget of widgets) {
        const b = widget.bounds;
        if (command.edge === "left") b.x = left;
        else if (command.edge === "right") b.x = right - b.width;
        else if (command.edge === "top") b.y = top;
        else if (command.edge === "bottom") b.y = bottom - b.height;
        else if (command.edge === "center-x") b.x = first.x + (first.width - b.width) / 2;
        else b.y = first.y + (first.height - b.height) / 2;
      } break;
    }
    case "translate": for (const widget of selected(command.ids)) { widget.bounds.x += command.dx; widget.bounds.y += command.dy; } break;
    case "snap": {
      const { columns, rows } = doc.presentation.grid;
      for (const widget of selected(command.ids)) {
        const region = widgetViewport(doc, widget)!;
        widget.bounds.x = Math.max(region.bounds.x, Math.min(region.bounds.x + region.bounds.width - widget.bounds.width, Math.round(widget.bounds.x * columns) / columns));
        widget.bounds.y = Math.max(region.bounds.y, Math.min(region.bounds.y + region.bounds.height - widget.bounds.height, Math.round(widget.bounds.y * rows) / rows));
      } break;
    }
    case "set-grid": doc.presentation.grid = { columns: command.columns, rows: command.rows, snap: command.snap }; break;
    case "set-viewport": {
      const { type: _, ...viewport } = command; const screen = screenFor(doc, command.pageId); Object.assign(screen, clone(viewport)); break;
    }
    case "add-viewport": {
      const screen = screenFor(doc, command.pageId), regions = viewportRegions(doc, command.pageId);
      assert(regions.length < DESIGNER_LIMITS.viewportRegions, "A companion screen supports at most 32 viewports");
      const source = command.sourceId ? regions.find(region => region.id === command.sourceId) : regions.at(-1);
      assert(source, "Unknown viewport to split");
      let bounds = freeViewportBounds(regions);
      if (!bounds) {
        const before = clone(source.bounds), horizontal = before.width * screen.width >= before.height * screen.height;
        source.bounds = horizontal ? { ...before, width: before.width / 2 } : { ...before, height: before.height / 2 };
        bounds = horizontal ? { ...before, x: before.x + before.width / 2, width: before.width / 2 } : { ...before, y: before.y + before.height / 2, height: before.height / 2 };
        transformViewportWidgets(doc, command.pageId, before, source.bounds);
      }
      regions.push({ id: newId("viewport"), label: "Viewport " + (regions.length + 1), bounds }); screen.regions = regions; break;
    }
    case "update-viewport": {
      const screen = screenFor(doc, command.pageId), regions = viewportRegions(doc, command.pageId), region = regions.find(region => region.id === command.id);
      assert(region, "Unknown viewport");
      assert(command.patch.id === undefined || command.patch.id === region.id, "Viewport IDs cannot be edited");
      assert(Object.keys(command.patch).every(key => ["id", "label", "bounds"].includes(key)), "Unexpected viewport fields");
      const before = clone(region.bounds); Object.assign(region, clone(command.patch));
      assertViewportBounds(region.bounds);
      assert(!regions.some(other => other.id !== region.id && boundsOverlap(other.bounds, region.bounds)), "Viewports must not overlap. Move or resize the viewport so their edges do not cross");
      if (command.patch.bounds) transformViewportWidgets(doc, command.pageId, before, region.bounds);
      screen.regions = regions; break;
    }
    case "remove-viewport": {
      const screen = screenFor(doc, command.pageId), regions = viewportRegions(doc, command.pageId), region = regions.find(region => region.id === command.id);
      assert(region, "Unknown viewport"); assert(regions.length > 1, "A companion screen needs at least one viewport");
      assert(!doc.presentation.widgets.some(widget => widget.pageId === command.pageId && boundsContain(region.bounds, widget.bounds)), "This viewport contains widgets. Move or delete its widgets before removing it");
      screen.regions = regions.filter(region => region.id !== command.id); break;
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

/** Keep editor geometry on whole physical pixels while retaining normalized storage. */
export function applyPixelDesignerCommand(history:DesignerHistory, command:DesignerCommand):DesignerHistory {
  const next=applyDesignerCommand(history,command),doc=next.present;
  for(const widget of doc.presentation.widgets){
    const screen=screenFor(doc,widget.pageId),region=widgetViewport(doc,widget)!;
    const width=screen.width*(1-(screen.safeInsets?.left??0)-(screen.safeInsets?.right??0));
    const height=screen.height*(1-(screen.safeInsets?.top??0)-(screen.safeInsets?.bottom??0));
    const align=(position:number,size:number,start:number,extent:number,scale:number)=>{
      const min=Math.ceil(start*scale-1e-8),max=Math.floor((start+extent)*scale+1e-8);
      assert(max>min,'Viewport must have room for a whole pixel');
      const pixels=Math.max(1,Math.min(max-min,Math.round(size*scale)));
      return [Math.max(min,Math.min(max-pixels,Math.round(position*scale)))/scale,pixels/scale];
    };
    const [x,w]=align(widget.bounds.x,widget.bounds.width,region.bounds.x,region.bounds.width,width);
    const [y,h]=align(widget.bounds.y,widget.bounds.height,region.bounds.y,region.bounds.height,height);
    widget.bounds={x,y,width:w,height:h};
  }
  doc.ui=normalizeToPortableUi(doc);next.present=checked(doc);
  return next;
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

/** Preserve legacy backgrounds as ordinary editable image widgets. */
export function migrateBackgroundImages(document:DesignerDocument):DesignerDocument {
  const doc=checked(document);
  for(const screen of doc.presentation.viewports){
    if(!screen.backgroundAssetId)continue;
    const images=viewportRegions(doc,screen.pageId).map(region=>({id:newId('widget'),pageId:screen.pageId,kind:'image' as const,label:'Image',bounds:{...region.bounds},controlIds:[],feedbackIds:[],config:{assetId:screen.backgroundAssetId}}));
    doc.presentation.widgets.unshift(...images);delete screen.backgroundAssetId;
  }
  return checked(doc);
}
