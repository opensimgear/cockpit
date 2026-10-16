import { applyDesignerCommand, createDesignerHistory, widgetPresets, type DesignerDocument } from '../../packages/companion-designer/src/index.ts';

const supported = new Set(['button', 'indicator', 'text', 'numeric', 'gauge']);
export const firmwareWidgetPresets = widgetPresets.filter(preset => supported.has(preset.kind));
/** Use editor commands so removed widgets also lose their controls and bindings. */
export function simplifyFirmwareWidgets(document: DesignerDocument) {
  let history = createDesignerHistory(document);
  const removed = document.presentation.widgets.filter(widget => !supported.has(widget.kind));
  for (const widget of removed) history = applyDesignerCommand(history, { type: 'delete-widget', id: widget.id });
  return { document: history.present, removed: removed.length };
}
