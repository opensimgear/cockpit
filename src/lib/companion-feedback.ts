export type FeedbackRouteSource = { kind: 'requested' | 'host-accepted' | 'host-submitted'; controlId: string } | { kind: 'integration'; bindingId: string };
export interface FeedbackRouteBinding { channelId: string; outputId: string; source: FeedbackRouteSource; scale?: number; offset?: number }
export interface FeedbackRouteOutput { outputId: string; channelId: string; kind: 'text' | 'indicator' | 'gauge'; unit: string; range?: { min: number; max: number } }
export interface FeedbackRouteInstance { instanceId: string; publicationHash: string; outputs: FeedbackRouteOutput[]; controls?: { id: string; label: string; kind: 'button' | 'axis' | 'hat'; axisMode?: string }[]; bindings: FeedbackRouteBinding[]; configured: boolean; pinned: boolean; error?: string | null }
export interface FeedbackRouteState { instances: FeedbackRouteInstance[]; sources: { bindingId: string; label: string; kind: string; unit: string; minimum?: number; maximum?: number; confirmed: boolean }[] }
const identifier = (value: string) => /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value);
/** Advisory checks mirror the declared immutable output shape; native authority remains required. */
export function feedbackBindingError(instance: FeedbackRouteInstance, bindings: FeedbackRouteBinding[], sources: FeedbackRouteState['sources']): string {
  if (!/^sha256:[a-f0-9]{64}$/.test(instance.publicationHash)) return 'Load a verified immutable publication first.';
  if (bindings.length > 64) return 'Use at most 64 feedback bindings per instance.';
  const used = new Set<string>();
  for (const binding of bindings) {
    const output = instance.outputs.find(output => output.channelId === binding.channelId && output.outputId === binding.outputId);
    if (!output || used.has(binding.channelId)) return 'Bind each published channel/output identity at most once.';
    used.add(binding.channelId);
    if (!Number.isFinite(binding.scale ?? 1) || Math.abs(binding.scale ?? 1) > 1e6 || !Number.isFinite(binding.offset ?? 0) || Math.abs(binding.offset ?? 0) > 1e9) return 'Use finite scale within ±1000000 and offset within ±1000000000.';
    let kind: string, unit: string, minimum: number|undefined, maximum: number|undefined;
    const origin=binding.source;
    if (origin.kind === 'integration') {
      const source = sources.find(source => source.bindingId === origin.bindingId);
      if (!source) return 'Choose an available logical telemetry binding.';
      kind = source.kind; unit = source.unit;minimum=typeof source.minimum==='number'&&Number.isFinite(source.minimum)?source.minimum:undefined;maximum=typeof source.maximum==='number'&&Number.isFinite(source.maximum)?source.maximum:undefined;
    } else {
      if (!['requested', 'host-accepted', 'host-submitted'].includes(binding.source.kind) || !identifier(origin.controlId)) return 'Choose a supported requested/host state and stable control identity.';
      const control = instance.controls?.find(control => control.id === origin.controlId);
      if (!control) return 'Choose a control from this exact published instance.';
      minimum=control.kind==='hat'?-1:control.axisMode==='bipolar'?-1:0;maximum=control.kind==='hat'?7:1;
      kind = control.kind === 'button' ? 'boolean' : 'number'; unit = control.kind === 'button' ? 'state' : control.kind === 'hat' ? 'direction' : 'ratio';
    }
    if (output.kind === 'indicator' ? kind !== 'boolean' : output.kind === 'text' ? kind !== 'string' : !['number','integer'].includes(kind)) return 'The source value type must match the published output widget.';
    if (output.unit !== unit && !(output.kind === 'gauge' && (binding.scale !== undefined || binding.offset !== undefined))) return 'Units differ; numeric conversion requires an explicit scale or offset.';
    if (output.range&&minimum!==undefined&&maximum!==undefined) {const endpoints=[minimum,maximum].map(value=>value*(binding.scale??1)+(binding.offset??0));if(endpoints.some(value=>!Number.isFinite(value)||value<output.range!.min||value>output.range!.max))return 'The converted source range must fit the immutable widget range.';}
    if (output.kind !== 'gauge' && ((binding.scale ?? 1) !== 1 || (binding.offset ?? 0) !== 0)) return 'Only numeric feedback supports scaling and offsets.';
  }
  return '';
}
export function feedbackSourceKey(source: FeedbackRouteSource): string { return `${source.kind}:${source.kind === 'integration' ? source.bindingId : source.controlId}`; }
export function feedbackSourceFromKey(value: string): FeedbackRouteSource | null {
  const split = value.indexOf(':'); if (split < 0) return null;
  const kind = value.slice(0, split), id = value.slice(split + 1);
  if (kind === 'integration') return /^[a-zA-Z0-9_.-]{1,31}$/.test(id) ? { kind, bindingId: id } : null;
  if (!identifier(id)) return null;
  return ['requested','host-accepted','host-submitted'].includes(kind) ? { kind: kind as 'requested' | 'host-accepted' | 'host-submitted', controlId: id } : null;
}
