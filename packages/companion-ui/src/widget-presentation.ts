import { currentWidgetSample, resolveWidgetLayers, resolveWidgetProperties, resolveWidgetState, type DesignerDocument, type DesignerWidget, type WidgetDefinition, type WidgetLayer, type WidgetSample } from '@opensimgear/companion-designer';
import type { FeedbackSamples } from './presentation.ts';

export function componentDefinition(document: DesignerDocument, widget: DesignerWidget): WidgetDefinition | undefined {
  return widget.kind === 'component' ? document.presentation.definitions?.find(d => d.id === widget.config.definitionId && d.version === widget.config.definitionVersion) : undefined;
}
/** Both Android and browser use this projection after host feedback validation. */
export function componentSamples(document: DesignerDocument, widget: DesignerWidget, definition: WidgetDefinition, samples: FeedbackSamples): Record<string, WidgetSample | undefined> {
  return Object.fromEntries(definition.outputs.map((port, index) => {
    const output = document.ui.feedbackWidgets.find(f => f.id === widget.feedbackIds[index]);
    const sample = output ? samples[output.channelId] : undefined;
    const typed = port.kind === 'indicator' ? typeof sample?.value === 'boolean' : port.kind === 'text' ? typeof sample?.value === 'string' : typeof sample?.value === 'number' && Number.isFinite(sample.value);
    const unit = port.unit === undefined || (port.unit.trim() ? port.unit : 'ratio') === sample?.unit;
    const sameRange = port.range ? sample?.range?.min === port.range.min && sample.range.max === port.range.max : sample?.range === undefined;
    const range = sameRange && (!sample?.range || typeof sample.value === 'number' && Number.isFinite(sample.range.min) && Number.isFinite(sample.range.max) && sample.range.min < sample.range.max && sample.value >= sample.range.min && sample.value <= sample.range.max);
    const identity = sample && sample.outputId === output?.id && sample.channelId === output?.channelId;
    return [port.name, identity && (sample.status === 'unknown' || sample.status === 'disconnected' || typed && unit && range) ? sample : undefined];
  }));
}
export function componentPresentation(document: DesignerDocument, widget: DesignerWidget, samples: FeedbackSamples, now: number, options: { pressed?: boolean; enabled?: boolean; connected?: boolean } = {}) {
  const definition = componentDefinition(document, widget);
  if (!definition || widget.kind !== 'component') return undefined;
  const outputs = componentSamples(document, widget, definition, samples);
  const enabledSample = definition.state.enabledPort ? currentWidgetSample(outputs[definition.state.enabledPort], now) : undefined;
  const interactionEnabled = !definition.state.enabledPort || enabledSample?.value === true;
  const state = resolveWidgetState(definition, { connected: options.connected ?? true, enabled: options.enabled ?? true, pressed: options.pressed ?? false, now, samples: outputs });
  return { definition, state, outputs, interactionEnabled, properties: resolveWidgetProperties(definition, widget.config.properties), layers: resolveWidgetLayers(definition, widget.config.properties, state) };
}
export function widgetLayerText(layer: WidgetLayer, samples: Record<string, WidgetSample | undefined>, now: number): string {
  if (layer.kind !== 'text') return '';
  if (!layer.outputPort) return String(layer.text);
  const sample = samples[layer.outputPort], current = currentWidgetSample(sample, now);
  if (!current) return sample?.status === 'stale' || sample && sample.status === 'fresh' ? 'STALE' : '—';
  return typeof current.value === 'number' ? `${current.value.toFixed(layer.precision ?? 0)}${layer.unit ? ` ${layer.unit}` : ''}` : String(current.value ?? '—').slice(0, 120);
}
export function widgetGaugeFraction(layer: WidgetLayer, samples: Record<string, WidgetSample | undefined>, now: number): number {
  if (layer.kind !== 'gauge') return 0;
  const sample = currentWidgetSample(samples[layer.outputPort], now), min = Number(layer.min), max = Number(layer.max);
  return sample && typeof sample.value === 'number' && max > min ? Math.max(0, Math.min(1, (sample.value - min) / (max - min))) : 0;
}
/** A confirmed indicator never lights from requested or historical state. */
export function widgetIndicatorActive(layer: WidgetLayer, samples: Record<string, WidgetSample | undefined>, now: number): boolean {
  if (layer.kind !== 'indicator') return false;
  const sample = currentWidgetSample(samples[layer.outputPort], now);
  return sample?.value === true && sample.provenance === 'game-confirmed' && sample.sourceKind === 'simulator';
}
export function radialGaugePath(x: number, y: number, width: number, height: number, fraction: number): string {
  const cx = x + width / 2, cy = y + height / 2, radius = Math.min(width, height) * .42;
  const start = 135 * Math.PI / 180, end = (135 + 270 * Math.max(.0001, Math.min(1, fraction))) * Math.PI / 180;
  return `M ${cx + radius * Math.cos(start)} ${cy + radius * Math.sin(start)} A ${radius} ${radius} 0 ${fraction > 2 / 3 ? 1 : 0} 1 ${cx + radius * Math.cos(end)} ${cy + radius * Math.sin(end)}`;
}
