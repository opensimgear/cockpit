import { canonical, newId } from './model.ts';
import { DESIGNER_LIMITS } from './types.ts';
import { validateWidgetDefinition, type WidgetDefinition, type WidgetLayer, type WidgetPaint, type WidgetProperty, type WidgetState } from './widget-model.ts';

export interface WidgetEditorHistory { past: WidgetDefinition[]; present: WidgetDefinition; future: WidgetDefinition[] }
export type WidgetEditorCommand =
  | { type: 'replace'; definition: WidgetDefinition }
  | { type: 'update'; patch: Partial<Pick<WidgetDefinition, 'title' | 'size' | 'hitArea' | 'state' | 'inputs' | 'outputs' | 'interaction'>> }
  | { type: 'add-layer'; kind: WidgetLayer['kind'] }
  | { type: 'update-layer'; id: string; patch: Record<string, unknown> }
  | { type: 'delete-layer' | 'duplicate-layer'; id: string }
  | { type: 'reorder-layer'; id: string; index: number }
  | { type: 'state-style'; id: string; state: WidgetState; paint: WidgetPaint & { text?: string | { property: string } } }
  | { type: 'set-property'; property: WidgetProperty }
  | { type: 'remove-property'; name: string };

function checked(definition: WidgetDefinition): WidgetDefinition {
  const r = validateWidgetDefinition(definition); if (!r.ok) throw new Error(r.error); return r.value;
}
export function createWidgetEditorHistory(definition: WidgetDefinition): WidgetEditorHistory { return { past: [], present: checked(definition), future: [] }; }
export function applyWidgetEditorCommand(history: WidgetEditorHistory, command: WidgetEditorCommand): WidgetEditorHistory {
  let definition = structuredClone(history.present);
  const layer = (id: string) => { const l = definition.layers.find(l => l.id === id); if (!l) throw new Error('Unknown widget layer'); return l; };
  switch (command.type) {
    case 'replace': definition = structuredClone(command.definition); break;
    case 'update':
      if (Object.keys(command.patch).some(key => !['title', 'size', 'hitArea', 'state', 'inputs', 'outputs', 'interaction'].includes(key))) throw new Error('Widget identity and revision are stable');
      Object.assign(definition, structuredClone(command.patch)); break;
    case 'add-layer': {
      if (!['rectangle', 'ellipse', 'line', 'text', 'image', 'indicator', 'gauge'].includes(command.kind)) throw new Error('Unsupported layer type');
      const id = newId('layer'), bounds = { x: .1, y: .1, width: .8, height: .8 }, base = { id, label: command.kind[0].toUpperCase() + command.kind.slice(1), bounds, fill: '#63dfd0' };
      if (command.kind === 'rectangle') definition.layers.push({ ...base, kind: 'rectangle', radius: 6 });
      else if (command.kind === 'ellipse' || command.kind === 'line') definition.layers.push({ ...base, kind: command.kind, stroke: '#63dfd0', strokeWidth: 2 });
      else if (command.kind === 'text') definition.layers.push({ ...base, kind: 'text', text: 'Label', font: 'sans', fontSize: 20, align: 'center', fill: '#ffffff' });
      else if (command.kind === 'image') definition.layers.push({ ...base, kind: 'image', assetId: '' });
      else {
        const kind = command.kind === 'indicator' ? 'indicator' : 'gauge';
        let port = definition.outputs.find(p => p.kind === kind);
        if (!port) { port = { name: newId('feedback'), label: kind === 'indicator' ? 'Status' : 'Value', kind, ...(kind === 'gauge' ? { unit: 'ratio', range: { min: 0, max: 1 } } : {}) }; definition.outputs.push(port); }
        definition.layers.push(command.kind === 'indicator' ? { ...base, kind: 'indicator', outputPort: port.name } : { ...base, kind: 'gauge', outputPort: port.name, min: port.range?.min ?? 0, max: port.range?.max ?? 100, orientation: 'horizontal', stroke: '#30434d', strokeWidth: 4 });
      }
      break;
    }
    case 'update-layer': {
      if (Object.keys(command.patch).some(key => ['id', 'kind'].includes(key))) throw new Error('Layer identity and type are stable');
      Object.assign(layer(command.id), structuredClone(command.patch)); break;
    }
    case 'delete-layer': layer(command.id); definition.layers = definition.layers.filter(l => l.id !== command.id); break;
    case 'duplicate-layer': { const l = structuredClone(layer(command.id)); l.id = newId('layer'); l.label = (l.label + ' copy').slice(0, 120); definition.layers.push(l); break; }
    case 'reorder-layer': {
      layer(command.id); if (!Number.isInteger(command.index) || command.index < 0 || command.index >= definition.layers.length) throw new Error('Layer order outside bounds');
      const index = definition.layers.findIndex(l => l.id === command.id), [l] = definition.layers.splice(index, 1); definition.layers.splice(command.index, 0, l); break;
    }
    case 'state-style': { const l = layer(command.id); l.states = { ...l.states, [command.state]: structuredClone(command.paint) }; break; }
    case 'set-property': {
      const i = definition.properties.findIndex(p => p.name === command.property.name);
      if (i < 0) definition.properties.push(structuredClone(command.property)); else definition.properties[i] = structuredClone(command.property); break;
    }
    case 'remove-property': {
      if (!definition.properties.some(p => p.name === command.name)) throw new Error('Unknown widget property');
      definition.properties = definition.properties.filter(p => p.name !== command.name); break;
    }
  }
  definition = checked(definition);
  if (canonical(definition) === canonical(history.present)) return history;
  return { past: [...history.past, structuredClone(history.present)].slice(-DESIGNER_LIMITS.history), present: definition, future: [] };
}
export function undoWidgetEditor(history: WidgetEditorHistory): WidgetEditorHistory {
  if (!history.past.length) return history;
  return { past: history.past.slice(0, -1), present: checked(history.past.at(-1)!), future: [structuredClone(history.present), ...history.future].slice(0, DESIGNER_LIMITS.history) };
}
export function redoWidgetEditor(history: WidgetEditorHistory): WidgetEditorHistory {
  if (!history.future.length) return history;
  return { past: [...history.past, structuredClone(history.present)].slice(-DESIGNER_LIMITS.history), present: checked(history.future[0]), future: history.future.slice(1) };
}
