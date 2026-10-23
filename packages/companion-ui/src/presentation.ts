import type { Bounds, FeedbackValue, PortableUi } from '@opensimgear/companion-contracts';
import type { DesignerWidget } from '@opensimgear/companion-designer';
export type FeedbackSamples = Record<string, FeedbackValue & { receivedAtMs: number }>;
export function safeAssetUrl(value: unknown): string | null {
  return typeof value === 'string' && value.length <= 3 * 1024 * 1024 && /^data:image\/(png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(value) ? value : null;
}
/** Output-only projection. A stale sample can show its last value but never light a live indicator. */
export function feedbackPresentation(widget: DesignerWidget, ui: PortableUi, samples: FeedbackSamples, now: number) {
  const source = ui.feedbackWidgets.find(item => item.id === widget.feedbackIds[0]);
  const sample = source ? samples[source.channelId] : undefined;
  const unavailable = (text = 'Unknown', freshness = 'unknown', provenance = 'No source') => ({ available: false, value: null, text, provenance, freshness, ageMs: Infinity, confirmed: false, unit: '' });
  if (!sample) return unavailable();
  if (!ui.feedbackWidgets.some(output => output.channelId === source?.channelId && output.id === sample.outputId)) return unavailable('Output identity mismatch');
  if (!Number.isFinite(now) || !Number.isFinite(sample.sourceAgeMs) || !Number.isFinite(sample.receivedAtMs) || sample.sourceAgeMs < 0) return unavailable('Invalid feedback');
  if (sample.status === 'unknown' || sample.status === 'disconnected') return unavailable(sample.status === 'unknown' ? 'Unknown' : 'Disconnected', sample.status, `${sample.provenance} unavailable · ${sample.sourceKind}`);
  const expected = source?.kind;
  const typed = expected === 'indicator' ? typeof sample.value === 'boolean' : expected === 'gauge' ? typeof sample.value === 'number' && Number.isFinite(sample.value) : typeof sample.value === 'string';
  if (!typed || !['fresh', 'stale'].includes(sample.status)) return unavailable(sample.status === 'stale' ? 'Stale · no value' : 'Invalid feedback', sample.status === 'stale' ? 'stale' : 'unknown');
  if (sample.range && (typeof sample.value !== 'number' || !Number.isFinite(sample.range.min) || !Number.isFinite(sample.range.max) || sample.range.min >= sample.range.max || sample.value < sample.range.min || sample.value > sample.range.max)) return unavailable('Range mismatch');
  if ((widget.kind === 'numeric' || widget.kind === 'gauge') && (widget.config.unit.trim() ? widget.config.unit : 'ratio') !== sample.unit) return unavailable('Unit mismatch');
  const ageMs = sample.sourceAgeMs + Math.max(0, now - sample.receivedAtMs);
  const freshness = sample.status === 'stale' || ageMs >= 1000 ? 'stale' : ageMs > 250 ? 'aging' : 'fresh';
  let text: string;
  if (widget.kind === 'numeric' || widget.kind === 'gauge') text = `${Number(sample.value).toFixed(widget.config.precision)}${sample.unit ? ` ${sample.unit}` : ''}`;
  else if (widget.kind === 'text') text = `${widget.config.prefix}${String(sample.value).slice(0, 256)}${widget.config.suffix}`;
  else text = sample.value ? 'Active' : 'Inactive';
  const current = freshness !== 'stale';
  return { available: current, value: current ? sample.value! : null, text: current ? text : `Stale · last ${text}`, provenance: `${sample.provenance}${current ? '' : ' (historical)'} · ${sample.sourceKind}`, freshness, ageMs, confirmed: current && sample.provenance === 'game-confirmed' && sample.sourceKind === 'simulator', unit: sample.unit };
}

export function movedBounds(bounds: {x:number;y:number;width:number;height:number}, dx:number, dy:number, grid: {columns:number;rows:number;snap:boolean}) {
  const x = bounds.x + dx, y = bounds.y + dy;
  return { ...bounds, x: Math.max(0, Math.min(1 - bounds.width, grid.snap ? Math.round(x * grid.columns) / grid.columns : x)), y: Math.max(0, Math.min(1 - bounds.height, grid.snap ? Math.round(y * grid.rows) / grid.rows : y)) };
}

export function resizedBounds(bounds: {x:number;y:number;width:number;height:number}, dx:number, dy:number, grid: {columns:number;rows:number;snap:boolean}) {
  const width = bounds.width + dx, height = bounds.height + dy;
  return { ...bounds, width: Math.max(Math.min(1 / grid.columns, 1 - bounds.x), Math.min(1 - bounds.x, grid.snap ? Math.round(width * grid.columns) / grid.columns : width)), height: Math.max(Math.min(1 / grid.rows, 1 - bounds.y), Math.min(1 - bounds.y, grid.snap ? Math.round(height * grid.rows) / grid.rows : height)) };
}

/** Preview the same proportional widget layout applied when its viewport moves or resizes. */
export function viewportWidgetBounds(bounds: Bounds, from: Bounds, to: Bounds): Bounds {
  const x = to.x + (bounds.x - from.x) / from.width * to.width;
  const y = to.y + (bounds.y - from.y) / from.height * to.height;
  return { x, y, width: Math.min(bounds.width / from.width * to.width, to.x + to.width - x, 1 - x), height: Math.min(bounds.height / from.height * to.height, to.y + to.height - y, 1 - y) };
}

export type ResizeCorner = 'nw' | 'ne' | 'sw' | 'se';

/** Fit authored artwork and its hit area together without changing proportions. */
export function fittedWidgetBounds(width:number,height:number,size:{width:number;height:number}):Bounds {
  if(width<=0||height<=0)return {x:0,y:0,width:1,height:1};
  const scale=Math.min(width/size.width,height/size.height);
  const w=size.width*scale/width,h=size.height*scale/height;
  return {x:(1-w)/2,y:(1-h)/2,width:w,height:h};
}
/** Move the dragged edges while leaving the opposite corner fixed. */
export function resizedCornerBounds(bounds:Bounds,dx:number,dy:number,grid:{columns:number;rows:number;snap:boolean},corner:ResizeCorner):Bounds {
  const snap=(value:number,steps:number)=>grid.snap?Math.round(value*steps)/steps:value;
  let left=bounds.x,top=bounds.y,right=left+bounds.width,bottom=top+bounds.height;
  if(corner.includes('w'))left=Math.max(0,Math.min(right-Math.min(1/grid.columns,right),snap(left+dx,grid.columns)));
  else right=Math.min(1,Math.max(left+Math.min(1/grid.columns,1-left),snap(right+dx,grid.columns)));
  if(corner.includes('n'))top=Math.max(0,Math.min(bottom-Math.min(1/grid.rows,bottom),snap(top+dy,grid.rows)));
  else bottom=Math.min(1,Math.max(top+Math.min(1/grid.rows,1-top),snap(bottom+dy,grid.rows)));
  return {x:left,y:top,width:right-left,height:bottom-top};
}
