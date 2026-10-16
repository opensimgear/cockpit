import { applyDesignerCommand, createDesignerHistory, newDocument } from "./editor.ts";
import { checked, normalizeToPortableUi } from "./model.ts";
import type { DesignerDocument, WidgetKind } from "./types.ts";
function template(title: string, kinds: WidgetKind[], prefix: string): DesignerDocument {
  let history = createDesignerHistory(newDocument(title));
  for (const kind of kinds) history = applyDesignerCommand(history, { type: "add-widget", kind });
  const doc = history.present, ids = new Map<string, string>();
  const remap = (id: string) => ids.get(id) ?? id;
  doc.ui.id = prefix + "-panel";
  for (const [index, control] of doc.ui.controls.entries()) { ids.set(control.id, prefix + "-input-" + index); control.id = remap(control.id); }
  for (const [index, output] of doc.ui.feedbackWidgets.entries()) { ids.set(output.id, prefix + "-output-" + index); output.id = remap(output.id); output.channelId = prefix + "-channel-" + index; }
  for (const [index, page] of doc.ui.pages.entries()) { ids.set(page.id, prefix + "-page-" + index); page.id = remap(page.id); }
  for (const [index, target] of doc.targets.entries()) { ids.set(target.id, prefix + "-target-" + index); target.id = remap(target.id); }
  for (const viewport of doc.presentation.viewports) viewport.pageId = remap(viewport.pageId);
  for (const [index, widget] of doc.presentation.widgets.entries()) {
    widget.id = prefix + "-widget-" + index; widget.pageId = remap(widget.pageId); widget.controlIds = widget.controlIds.map(remap); widget.feedbackIds = widget.feedbackIds.map(remap);
    widget.bounds = { x: 0.02 + index % 3 * 0.33, y: 0.02 + Math.floor(index / 3) * 0.24, width: 0.30, height: 0.21 };
  }
  for (const binding of doc.bindings) { binding.controlId = remap(binding.controlId); binding.targetId = remap(binding.targetId); }
  if (prefix === "flight") {
    const buttons = doc.presentation.widgets.filter(widget => widget.kind === "button");
    if (buttons[1]?.kind === "button") buttons[1].config.buttonMode = "toggle";
  } else {
    const slider = doc.presentation.widgets.find(widget => widget.kind === "slider");
    const control = doc.ui.controls.find(control => control.id === slider?.controlIds[0]);
    if (control?.kind === "axis") {
      control.axisMode = "unipolar"; const binding = doc.bindings.find(binding => binding.controlId === control.id)!;
      doc.targets.find(target => target.id === binding.targetId)!.axes[binding.index] = "unipolar";
    }
  }
  doc.ui = normalizeToPortableUi(doc); return checked(doc);
}
export const flightTemplate = template("Flight instruments", ["button", "xy", "selector", "encoder", "hat", "slider", "indicator", "text", "numeric", "icon", "gauge"], "flight");
export const racingTemplate = template("Racing dashboard", ["slider", "xy", "encoder", "indicator", "numeric", "gauge"], "racing");
