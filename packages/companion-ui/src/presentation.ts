import type { FeedbackValue, PortableUi } from '@opensimgear/companion-contracts';
import type { DesignerWidget } from '@opensimgear/companion-designer';
export type FeedbackSamples = Record<string, FeedbackValue & { receivedAtMs: number }>;
export function safeAssetUrl(value: unknown): string | null {
  return typeof value === 'string' && value.length <= 3 * 1024 * 1024 && /^data:image\/(png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(value) ? value : null;
}
export function feedbackPresentation(widget: DesignerWidget, ui: PortableUi, samples: FeedbackSamples, now: number) {
  const source = ui.feedbackWidgets.find(item => item.id === widget.feedbackIds[0]);
  const sample = source ? samples[source.channelId] : undefined;
  const expected = source?.kind;
  const valid = sample && Number.isFinite(sample.sourceAgeMs) && Number.isFinite(sample.receivedAtMs) && sample.sourceAgeMs >= 0 && (
    expected === 'indicator' ? typeof sample.value === 'boolean' : expected === 'gauge' ? typeof sample.value === 'number' && Number.isFinite(sample.value) : typeof sample.value === 'string'
  );
  if (!valid) return { available: false, value: null, text: 'Unavailable', provenance: 'No source', freshness: 'unavailable', ageMs: Infinity };
  const ageMs = sample.sourceAgeMs + Math.max(0, now - sample.receivedAtMs);
  const freshness = ageMs > 1000 ? 'stale' : ageMs > 250 ? 'aging' : 'fresh';
  let text: string;
  if (widget.kind === 'numeric' || widget.kind === 'gauge') text = `${Number(sample.value).toFixed(widget.config.precision)}${widget.config.unit ? ` ${widget.config.unit}` : ''}`;
  else if (widget.kind === 'text') text = `${widget.config.prefix}${String(sample.value).slice(0, 1024)}${widget.config.suffix}`;
  else text = sample.value ? 'Active' : 'Inactive';
  return { available: true, value: sample.value, text, provenance: `${sample.provenance} · ${sample.sourceKind}`, freshness, ageMs };
}

export function movedBounds(bounds: {x:number;y:number;width:number;height:number}, dx:number, dy:number, grid: {columns:number;rows:number;snap:boolean}) {
  const x = bounds.x + dx, y = bounds.y + dy;
  return { ...bounds, x: Math.max(0, Math.min(1 - bounds.width, grid.snap ? Math.round(x * grid.columns) / grid.columns : x)), y: Math.max(0, Math.min(1 - bounds.height, grid.snap ? Math.round(y * grid.rows) / grid.rows : y)) };
}

export function resizedBounds(bounds: {x:number;y:number;width:number;height:number}, dx:number, dy:number, grid: {columns:number;rows:number;snap:boolean}) {
  const width = bounds.width + dx, height = bounds.height + dy;
  return { ...bounds, width: Math.max(Math.min(1 / grid.columns, 1 - bounds.x), Math.min(1 - bounds.x, grid.snap ? Math.round(width * grid.columns) / grid.columns : width)), height: Math.max(Math.min(1 / grid.rows, 1 - bounds.y), Math.min(1 - bounds.y, grid.snap ? Math.round(height * grid.rows) / grid.rows : height)) };
}
