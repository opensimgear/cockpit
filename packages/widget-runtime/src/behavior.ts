/** Bounded, data-only behavior shared by compiled players and the authoring simulator. */
export type Scalar = boolean | number | string;
export type Expression =
  | { literal: Scalar }
  | { ref: string }
  | { state: string }
  | { op: 'not'; value: Expression }
  | { op: 'and' | 'or'; values: Expression[] }
  | { op: 'eq' | 'ne' | 'lt' | 'lte' | 'gt' | 'gte' | 'add' | 'sub' | 'mul' | 'div'; left: Expression; right: Expression }
  | { op: 'clamp'; value: Expression; min: Expression; max: Expression }
  | { op: 'if'; condition: Expression; then: Expression; else: Expression };

type Domain = { type: 'boolean' } | { type: 'number'; min: number; max: number } | { type: 'enum'; values: string[] };
export type BehaviorSignal = Domain & {
  id: string; source: 'local' | 'input' | 'feedback'; initial: Scalar;
  /** Feedback age includes its age at the source, rather than arrival alone. */
  freshnessMs?: number; qualitySignal?: string; requireGameConfirmed?: boolean;
};
export type BehaviorOutput = Domain & { id: string; neutral: Scalar };
export type BehaviorAction =
  | { type: 'set'; signal: string; value: Expression }
  | { type: 'emit'; port: string; value: Expression }
  | { type: 'cue'; cue: string }
  | { type: 'start-timer' | 'cancel-timer'; timer: string };
export interface BehaviorTransition {
  id: string; from: string; event: string; priority?: number; guard?: Expression;
  to?: string; actions?: BehaviorAction[];
}
export interface BehaviorRegion { id: string; initial: string; states: string[]; transitions: BehaviorTransition[] }
export interface BehaviorDefinition {
  version: 1; signals: BehaviorSignal[];
  events: { id: string; updates?: string[] }[];
  regions: BehaviorRegion[];
  outputs?: BehaviorOutput[];
  timers?: { id: string; event: string; delayMs: number; repeat?: boolean }[];
}
export type FeedbackQuality = 'fresh' | 'unknown' | 'stale' | 'disconnected';
export interface BehaviorFeedback {
  status: FeedbackQuality; value?: Scalar; sourceAgeMs?: number;
  provenance?: 'requested' | 'local' | 'host-accepted' | 'host-submitted' | 'game-confirmed';
  sourceKind?: 'simulator' | 'host' | 'script';
}
export interface BehaviorState {
  values: Record<string, Scalar>; regions: Record<string, string>; nowMs: number;
  timers: { id: string; dueMs: number }[];
  feedback: Record<string, BehaviorFeedback & { receivedAtMs: number }>;
}
export type BehaviorEvent =
  | { type: 'event'; event: string; nowMs: number; updates?: Record<string, Scalar>; feedback?: Record<string, BehaviorFeedback>; suppressCues?: boolean }
  | { type: 'tick'; nowMs: number }
  | { type: 'reset'; nowMs: number };
export type BehaviorEffect = { type: 'emit'; port: string; value: Scalar } | { type: 'cue'; cue: string };
export interface BehaviorTrace { event: string; atMs: number; region: string; transition: string; from: string; to: string }
export interface BehaviorResult { state: BehaviorState; effects: BehaviorEffect[]; trace: BehaviorTrace[]; error?: string }
export interface ExpressionContext { values: Readonly<Record<string, Scalar>>; regions: Readonly<Record<string, string>> }

export const BEHAVIOR_LIMITS = Object.freeze({
  bytes: 131072, signals: 32, events: 32, regions: 8, states: 32, transitions: 128,
  actions: 8, timers: 16, outputs: 16, expressionNodes: 32, expressionDepth: 8,
  work: 32768, timerFirings: 256, milliseconds: 86400000,
});
type Validation<T> = { ok: true; value: T } | { ok: false; error: string };
type Kind = 'boolean' | 'number' | 'string';
interface Inference { kind: Kind; min?: number; max?: number; values?: string[] }
function ensure(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function object(value: unknown, required: string[], optional: string[] = []): Record<string, unknown> {
  ensure(value !== null && typeof value === 'object' && !Array.isArray(value) && Object.getPrototypeOf(value) === Object.prototype, 'Expected a plain behavior object');
  const item = value as Record<string, unknown>;
  ensure(required.every(key => Object.hasOwn(item, key)) && Object.keys(item).every(key => required.includes(key) || optional.includes(key)), 'Unexpected or missing behavior field');
  return item;
}
function id(value: unknown): asserts value is string { ensure(typeof value === 'string' && /^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(value), 'Invalid behavior ID'); }
function number(value: unknown, min = -1e9, max = 1e9): asserts value is number { ensure(typeof value === 'number' && Number.isFinite(value) && value >= min && value <= max, 'Behavior number outside bounds'); }
function integer(value: unknown, min: number, max: number): asserts value is number { number(value, min, max); ensure(Number.isSafeInteger(value), 'Expected a behavior integer'); }
function list(value: unknown, maximum: number, minimum = 0): unknown[] { ensure(Array.isArray(value) && value.length >= minimum && value.length <= maximum, 'Behavior list outside bounds'); return value; }
function unique(items: { id: string }[]): void { const seen = new Set<string>(); for (const item of items) { id(item.id); ensure(!seen.has(item.id), 'Duplicate behavior ID'); seen.add(item.id); } }
function scalar(value: unknown): asserts value is Scalar { ensure(typeof value === 'boolean' || typeof value === 'number' && Number.isFinite(value) && Math.abs(value) <= 1e9 || typeof value === 'string' && value.length <= 120 && !/[\u0000-\u001f]/.test(value), 'Invalid behavior scalar'); }
function accepts(domain: Domain, value: unknown): boolean {
  return domain.type === 'boolean' ? typeof value === 'boolean' : domain.type === 'number' ? typeof value === 'number' && Number.isFinite(value) && value >= domain.min && value <= domain.max : typeof value === 'string' && domain.values.includes(value);
}
function domain(item: Record<string, unknown>, value: unknown): void {
  ensure(['boolean', 'number', 'enum'].includes(String(item.type)), 'Invalid behavior signal type');
  if (item.type === 'number') { number(item.min); number(item.max); ensure(item.min < item.max, 'Invalid behavior numeric domain'); }
  if (item.type === 'enum') { const values = list(item.values, 32, 1); values.forEach(id); ensure(new Set(values).size === values.length, 'Repeated behavior enum value'); }
  ensure(accepts(item as unknown as Domain, value), 'Behavior initial/neutral outside domain');
}
function domainInference(value: Domain): Inference { return value.type === 'enum' ? { kind: 'string', values: value.values } : value.type === 'number' ? { kind: 'number', min: value.min, max: value.max } : { kind: 'boolean' }; }
function infer(expression: unknown, definition: BehaviorDefinition, budget = { nodes: 0 }, depth = 0): Inference {
  ensure(depth < BEHAVIOR_LIMITS.expressionDepth && ++budget.nodes <= BEHAVIOR_LIMITS.expressionNodes, 'Behavior expression exceeds work bounds');
  ensure(expression !== null && typeof expression === 'object' && !Array.isArray(expression), 'Invalid behavior expression');
  const e = expression as Record<string, unknown>;
  if (Object.hasOwn(e, 'literal')) { object(e, ['literal']); scalar(e.literal); return typeof e.literal === 'number' ? { kind: 'number', min: e.literal, max: e.literal } : typeof e.literal === 'string' ? { kind: 'string', values: [e.literal] } : { kind: 'boolean' }; }
  if (Object.hasOwn(e, 'ref')) { object(e, ['ref']); const signal = definition.signals.find(s => s.id === e.ref); ensure(signal, 'Unknown behavior signal reference'); return domainInference(signal); }
  if (Object.hasOwn(e, 'state')) { object(e, ['state']); const region = definition.regions.find(r => r.id === e.state); ensure(region, 'Unknown behavior region reference'); return { kind: 'string', values: region.states }; }
  const child = (value: unknown) => infer(value, definition, budget, depth + 1);
  if (e.op === 'not') { object(e, ['op', 'value']); ensure(child(e.value).kind === 'boolean', 'Boolean expression required'); return { kind: 'boolean' }; }
  if (e.op === 'and' || e.op === 'or') { object(e, ['op', 'values']); for (const value of list(e.values, 16, 1)) ensure(child(value).kind === 'boolean', 'Boolean expression required'); return { kind: 'boolean' }; }
  if (e.op === 'if') {
    object(e, ['op', 'condition', 'then', 'else']); ensure(child(e.condition).kind === 'boolean', 'Boolean expression required');
    const a = child(e.then), b = child(e.else); ensure(a.kind === b.kind, 'Conditional branches have different types');
    return a.kind === 'number' ? { kind: a.kind, min: Math.min(a.min!, b.min!), max: Math.max(a.max!, b.max!) } : a.kind === 'string' ? { kind: a.kind, values: [...new Set([...a.values!, ...b.values!])] } : a;
  }
  if (e.op === 'clamp') {
    object(e, ['op', 'value', 'min', 'max']); const v = child(e.value), low = child(e.min), high = child(e.max);
    ensure(v.kind === 'number' && low.kind === 'number' && high.kind === 'number' && low.max! <= high.min!, 'Invalid numeric clamp bounds');
    return { kind: 'number', min: low.min, max: high.max };
  }
  object(e, ['op', 'left', 'right']);
  ensure(['eq', 'ne', 'lt', 'lte', 'gt', 'gte', 'add', 'sub', 'mul', 'div'].includes(String(e.op)), 'Unsupported behavior operator');
  const a = child(e.left), b = child(e.right);
  if (e.op === 'eq' || e.op === 'ne') { ensure(a.kind === b.kind, 'Comparison types differ'); return { kind: 'boolean' }; }
  ensure(a.kind === 'number' && b.kind === 'number', 'Numeric expression required');
  if (['lt', 'lte', 'gt', 'gte'].includes(String(e.op))) return { kind: 'boolean' };
  let min: number, max: number;
  if (e.op === 'add') { min = a.min! + b.min!; max = a.max! + b.max!; }
  else if (e.op === 'sub') { min = a.min! - b.max!; max = a.max! - b.min!; }
  else {
    ensure(e.op !== 'div' || b.min! > 0 || b.max! < 0, 'Division domain includes zero');
    const results = [a.min!, a.max!].flatMap(x => [b.min!, b.max!].map(y => e.op === 'mul' ? x * y : x / y)); min = Math.min(...results); max = Math.max(...results);
  }
  number(min); number(max); return { kind: 'number', min, max };
}
function assignable(inference: Inference, target: Domain): boolean {
  return target.type === 'boolean' ? inference.kind === 'boolean' : target.type === 'number' ? inference.kind === 'number' && inference.min! >= target.min && inference.max! <= target.max : inference.kind === 'string' && inference.values!.every(value => target.values.includes(value));
}

/** Strictly validates and detaches untrusted imported behavior data. No executable code is accepted. */
export function validateBehaviorDefinition(input: unknown): Validation<BehaviorDefinition> {
  try {
    const json = JSON.stringify(input); ensure(json && new TextEncoder().encode(json).length <= BEHAVIOR_LIMITS.bytes, 'Behavior exceeds byte budget');
    const d = object(JSON.parse(json), ['version', 'signals', 'events', 'regions'], ['outputs', 'timers']) as unknown as BehaviorDefinition;
    ensure(d.version === 1, 'Unsupported behavior version');
    list(d.signals, BEHAVIOR_LIMITS.signals); list(d.events, BEHAVIOR_LIMITS.events); list(d.regions, BEHAVIOR_LIMITS.regions); list(d.outputs ?? [], BEHAVIOR_LIMITS.outputs); list(d.timers ?? [], BEHAVIOR_LIMITS.timers);
    unique(d.signals); unique(d.events); unique(d.regions); unique(d.outputs ?? []); unique(d.timers ?? []);
    for (const signal of d.signals) {
      const keys = signal.type === 'number' ? ['min', 'max'] : signal.type === 'enum' ? ['values'] : [];
      object(signal, ['id', 'source', 'type', 'initial', ...keys], ['freshnessMs', 'qualitySignal', 'requireGameConfirmed']); domain(signal as unknown as Record<string, unknown>, signal.initial);
      ensure(['local', 'input', 'feedback'].includes(signal.source), 'Invalid behavior signal source');
      if (signal.source !== 'feedback') ensure(signal.freshnessMs === undefined && signal.qualitySignal === undefined && signal.requireGameConfirmed === undefined, 'Feedback options require a feedback signal');
      else {
        integer(signal.freshnessMs ?? 1000, 1, BEHAVIOR_LIMITS.milliseconds);
        if (signal.requireGameConfirmed !== undefined) ensure(typeof signal.requireGameConfirmed === 'boolean', 'Invalid confirmation requirement');
        if (signal.qualitySignal !== undefined) { const quality = d.signals.find(s => s.id === signal.qualitySignal); ensure(quality?.source === 'input' && quality.type === 'enum' && ['fresh', 'unknown', 'stale', 'disconnected'].every(value => quality.values.includes(value)) && quality.initial === 'unknown', 'Invalid feedback quality signal'); ensure(d.signals.filter(s => s.qualitySignal === quality.id).length === 1, 'Shared feedback quality writer'); }
      }
    }
    for (const output of d.outputs ?? []) { const keys = output.type === 'number' ? ['min', 'max'] : output.type === 'enum' ? ['values'] : []; object(output, ['id', 'type', 'neutral', ...keys]); domain(output as unknown as Record<string, unknown>, output.neutral); }
    for (const event of d.events) { object(event, ['id'], ['updates']); const updates = list(event.updates ?? [], BEHAVIOR_LIMITS.signals); ensure(new Set(updates).size === updates.length, 'Repeated event update'); for (const signal of updates) ensure(d.signals.some(s => s.id === signal && s.source !== 'local' && !d.signals.some(f => f.qualitySignal === s.id)), 'Event updates require an external signal'); }
    for (const timer of d.timers ?? []) { object(timer, ['id', 'event', 'delayMs'], ['repeat']); ensure(d.events.some(e => e.id === timer.event), 'Unknown timer event'); integer(timer.delayMs, 1, BEHAVIOR_LIMITS.milliseconds); if (timer.repeat !== undefined) ensure(typeof timer.repeat === 'boolean', 'Invalid timer repetition'); }
    let count = 0;
    for (const region of d.regions) {
      object(region, ['id', 'initial', 'states', 'transitions']); const states = list(region.states, BEHAVIOR_LIMITS.states, 1); states.forEach(id); ensure(new Set(states).size === states.length && states.includes(region.initial), 'Invalid initial or duplicate region state');
      list(region.transitions, BEHAVIOR_LIMITS.transitions); count += region.transitions.length; unique(region.transitions);
      const priorities: BehaviorTransition[] = [];
      for (const transition of region.transitions) {
        object(transition, ['id', 'from', 'event'], ['priority', 'guard', 'to', 'actions']); ensure(transition.from === '*' || states.includes(transition.from), 'Unknown transition source'); ensure(d.events.some(e => e.id === transition.event), 'Unknown transition event');
        if (transition.to !== undefined) ensure(states.includes(transition.to), 'Unknown transition destination'); integer(transition.priority ?? 0, -32768, 32767);
        ensure(!priorities.some(other => other.event === transition.event && (other.from === transition.from || other.from === '*' || transition.from === '*') && (other.priority ?? 0) === (transition.priority ?? 0)), 'Ambiguous transition priority'); priorities.push(transition);
        if (transition.guard !== undefined) ensure(infer(transition.guard, d).kind === 'boolean', 'Transition guard must be boolean');
        const writes = new Set<string>();
        for (const action of list(transition.actions ?? [], BEHAVIOR_LIMITS.actions) as BehaviorAction[]) {
          if (action.type === 'set' || action.type === 'emit') {
            object(action, action.type === 'set' ? ['type', 'signal', 'value'] : ['type', 'port', 'value']);
            const target = action.type === 'set' ? d.signals.find(s => s.id === action.signal && s.source === 'local') : d.outputs?.find(o => o.id === action.port);
            ensure(target, 'Unknown or external action target'); ensure(assignable(infer(action.value, d), target), 'Action expression outside target type/range');
            const key = `${action.type}:${action.type === 'set' ? action.signal : action.port}`; ensure(!writes.has(key), 'Repeated transition write'); writes.add(key);
          } else if (action.type === 'cue') { object(action, ['type', 'cue']); id(action.cue); }
          else { object(action, ['type', 'timer']); ensure(['start-timer', 'cancel-timer'].includes(action.type) && d.timers?.some(timer => timer.id === action.timer), 'Unknown timer action'); const key = `timer:${action.timer}`; ensure(!writes.has(key), 'Repeated timer action'); writes.add(key); }
        }
      }
    }
    ensure(count <= BEHAVIOR_LIMITS.transitions, 'Behavior transition budget exceeded'); return { ok: true, value: d };
  } catch (error) { return { ok: false, error: error instanceof Error ? error.message : 'Invalid behavior' }; }
}

function evaluate(expression: Expression, context: ExpressionContext, work: { count: number }, depth = 0, nodes = { count: 0 }): Scalar {
  ensure(++work.count <= BEHAVIOR_LIMITS.work && ++nodes.count <= BEHAVIOR_LIMITS.expressionNodes && depth < BEHAVIOR_LIMITS.expressionDepth, 'Behavior evaluation work exceeded');
  const child = (value: Expression) => evaluate(value, context, work, depth + 1, nodes);
  let result: Scalar;
  if ('literal' in expression) result = expression.literal;
  else if ('ref' in expression) { ensure(Object.hasOwn(context.values, expression.ref), 'Missing behavior signal value'); result = context.values[expression.ref]; }
  else if ('state' in expression) { ensure(Object.hasOwn(context.regions, expression.state), 'Missing behavior region state'); result = context.regions[expression.state]; }
  else if (expression.op === 'not') { const value = child(expression.value); ensure(typeof value === 'boolean', 'Boolean expression required'); result = !value; }
  else if (expression.op === 'and' || expression.op === 'or') { list(expression.values, 16, 1); const values = expression.values.map(child); ensure(values.every(v => typeof v === 'boolean'), 'Boolean expression required'); result = expression.op === 'and' ? values.every(Boolean) : values.some(Boolean); }
  else if (expression.op === 'if') { const condition = child(expression.condition); ensure(typeof condition === 'boolean', 'Boolean condition required'); result = child(condition ? expression.then : expression.else); }
  else if (expression.op === 'clamp') { const value = child(expression.value), min = child(expression.min), max = child(expression.max); ensure(typeof value === 'number' && typeof min === 'number' && typeof max === 'number' && min <= max, 'Invalid clamp values'); result = Math.max(min, Math.min(max, value)); }
  else {
    ensure('left' in expression && 'right' in expression, 'Invalid binary behavior expression');
    const a = child(expression.left), b = child(expression.right);
    if (expression.op === 'eq' || expression.op === 'ne') { ensure(typeof a === typeof b, 'Comparison types differ'); result = expression.op === 'eq' ? a === b : a !== b; }
    else {
      ensure(typeof a === 'number' && typeof b === 'number', 'Numeric expression required');
      switch (expression.op) {
        case 'lt': result = a < b; break; case 'lte': result = a <= b; break; case 'gt': result = a > b; break; case 'gte': result = a >= b; break;
        case 'add': result = a + b; break; case 'sub': result = a - b; break; case 'mul': result = a * b; break;
        case 'div': ensure(b !== 0, 'Division by zero'); result = a / b; break;
        default: throw new Error('Unsupported behavior operator');
      }
    }
  }
  scalar(result); return result;
}
/** Also used by the compiler for finite presentation bindings. */
export function evaluateExpression(expression: Expression, context: ExpressionContext): Scalar { return evaluate(expression, context, { count: 0 }); }

function checked(definition: BehaviorDefinition): BehaviorDefinition { const result = validateBehaviorDefinition(definition); ensure(result.ok, result.ok ? '' : result.error); return result.value; }
export function createBehaviorState(definition: BehaviorDefinition, nowMs = 0): BehaviorState {
  const d = checked(definition); integer(nowMs, 0, Number.MAX_SAFE_INTEGER);
  return { values: Object.fromEntries(d.signals.map(signal => [signal.id, signal.initial])), regions: Object.fromEntries(d.regions.map(region => [region.id, region.initial])), nowMs, timers: [], feedback: {} };
}
function clone(state: BehaviorState): BehaviorState { return { values: { ...state.values }, regions: { ...state.regions }, nowMs: state.nowMs, timers: state.timers.map(timer => ({ ...timer })), feedback: Object.fromEntries(Object.entries(state.feedback).map(([key, value]) => [key, { ...value }])) }; }
function freshen(d: BehaviorDefinition, state: BehaviorState, nowMs: number): void {
  for (const signal of d.signals) if (signal.source === 'feedback') {
    const sample = state.feedback[signal.id]; let status = sample?.status ?? 'unknown';
    if (status === 'fresh' && sample && (sample.sourceAgeMs ?? 0) + nowMs - sample.receivedAtMs >= (signal.freshnessMs ?? 1000)) status = 'stale';
    state.values[signal.id] = status === 'fresh' && sample?.value !== undefined ? sample.value : signal.initial;
    if (signal.qualitySignal) state.values[signal.qualitySignal] = status;
  }
}
function admitFeedback(signal: BehaviorSignal, sample: BehaviorFeedback, nowMs: number): BehaviorFeedback & { receivedAtMs: number } {
  object(sample, ['status'], ['value', 'sourceAgeMs', 'provenance', 'sourceKind']);
  ensure(['fresh', 'unknown', 'stale', 'disconnected'].includes(sample.status), 'Invalid feedback quality'); integer(sample.sourceAgeMs ?? 0, 0, BEHAVIOR_LIMITS.milliseconds);
  if (sample.provenance !== undefined) ensure(['requested', 'local', 'host-accepted', 'host-submitted', 'game-confirmed'].includes(sample.provenance), 'Invalid feedback provenance');
  if (sample.sourceKind !== undefined) ensure(['simulator', 'host', 'script'].includes(sample.sourceKind), 'Invalid feedback source');
  ensure(sample.provenance !== 'game-confirmed' || sample.sourceKind === 'simulator', 'Game confirmation requires a simulator source');
  ensure(sample.status !== 'fresh' || accepts(signal, sample.value), 'Fresh feedback has invalid value');
  ensure(!['unknown', 'disconnected'].includes(sample.status) || sample.value === undefined, 'Unknown feedback cannot carry a value');
  if (sample.value !== undefined) ensure(accepts(signal, sample.value), 'Feedback outside signal domain');
  const result = { ...sample, receivedAtMs: nowMs };
  if (sample.status === 'fresh' && signal.requireGameConfirmed && (sample.provenance !== 'game-confirmed' || sample.sourceKind !== 'simulator')) { result.status = 'unknown'; delete result.value; }
  return result;
}

/** Events are atomic: all regions read one snapshot; conflicting writes reject the whole event. */
export function reduceBehavior(definition: BehaviorDefinition, previous: BehaviorState, event: BehaviorEvent): BehaviorResult {
  try {
    const d = checked(definition); object(event, ['type', 'nowMs'], event.type === 'event' ? ['event', 'updates', 'feedback', 'suppressCues'] : []);
    if (event.type === 'event' && event.suppressCues !== undefined) ensure(typeof event.suppressCues === 'boolean', 'Invalid cue suppression');
    integer(event.nowMs, 0, Number.MAX_SAFE_INTEGER); ensure(event.nowMs >= previous.nowMs, 'Non-monotonic behavior clock');
    if (event.type === 'reset') return { state: createBehaviorState(d, event.nowMs), effects: (d.outputs ?? []).map(output => ({ type: 'emit', port: output.id, value: output.neutral })), trace: [] };
    object(previous, ['values', 'regions', 'nowMs', 'timers', 'feedback']); integer(previous.nowMs, 0, Number.MAX_SAFE_INTEGER);
    object(previous.values, d.signals.map(s => s.id)); object(previous.regions, d.regions.map(r => r.id)); object(previous.feedback, [], d.signals.filter(s => s.source === 'feedback').map(s => s.id));
    const state = clone(previous); const effects: BehaviorEffect[] = [], trace: BehaviorTrace[] = []; const work = { count: 0 };
    ensure(Object.keys(state.values).length === d.signals.length && d.signals.every(signal => accepts(signal, state.values[signal.id])) && Object.keys(state.regions).length === d.regions.length && d.regions.every(region => region.states.includes(state.regions[region.id])), 'Invalid behavior snapshot');
    ensure(state.timers.length <= BEHAVIOR_LIMITS.timers && new Set(state.timers.map(timer => timer.id)).size === state.timers.length && state.timers.every(timer => d.timers?.some(t => t.id === timer.id) && Number.isSafeInteger(timer.dueMs) && timer.dueMs > previous.nowMs), 'Invalid behavior timers');
    for (const [key, sample] of Object.entries(state.feedback)) {
      const signal = d.signals.find(s => s.id === key && s.source === 'feedback'); ensure(signal, 'Unknown feedback snapshot signal');
      const { receivedAtMs, ...feedback } = sample; integer(receivedAtMs, 0, previous.nowMs); const admitted = admitFeedback(signal, feedback, receivedAtMs);
      ensure(admitted.status === sample.status && admitted.value === sample.value, 'Untrusted feedback snapshot');
    }
    function dispatch(eventId: string, atMs: number, suppressCues = false): void {
      freshen(d, state, atMs); const snapshot = clone(state); const writes = new Set<string>(); const pending: (() => void)[] = [];
      function claim(key: string) { ensure(!writes.has(key), 'Conflicting writes across behavior regions'); writes.add(key); }
      for (const region of d.regions) {
        const selected = region.transitions.filter(t => t.event === eventId && (t.from === '*' || t.from === snapshot.regions[region.id])).sort((a, b) => (b.priority ?? 0) - (a.priority ?? 0)).find(t => !t.guard || evaluate(t.guard, snapshot, work) === true);
        if (!selected) continue;
        const from = snapshot.regions[region.id], to = selected.to ?? from;
        pending.push(() => { state.regions[region.id] = to; trace.push({ event: eventId, atMs, region: region.id, transition: selected.id, from, to }); });
        for (const action of selected.actions ?? []) {
          if (action.type === 'set') { claim(`value:${action.signal}`); const target = d.signals.find(s => s.id === action.signal)!; const value = evaluate(action.value, snapshot, work); ensure(accepts(target, value), 'Action result outside signal domain'); pending.push(() => { state.values[action.signal] = value; }); }
          else if (action.type === 'emit') { claim(`output:${action.port}`); const value = evaluate(action.value, snapshot, work); const output = d.outputs!.find(o => o.id === action.port)!; ensure(accepts(output, value), 'Emitted value outside output domain'); pending.push(() => effects.push({ type: 'emit', port: action.port, value })); }
          else if (action.type === 'cue') { if (!suppressCues) pending.push(() => effects.push({ type: 'cue', cue: action.cue })); }
          else { claim(`timer:${action.timer}`); pending.push(() => { state.timers = state.timers.filter(t => t.id !== action.timer); if (action.type === 'start-timer') { const timer = d.timers!.find(t => t.id === action.timer)!; const dueMs = atMs + timer.delayMs; ensure(Number.isSafeInteger(dueMs), 'Behavior timer clock overflow'); state.timers.push({ id: timer.id, dueMs }); } }); }
        }
      }
      pending.forEach(apply => apply());
    }
    // Due timers precede an external event at the same timestamp, ordered by declaration.
    let firings = 0;
    while (true) {
      const due = state.timers.filter(t => t.dueMs <= event.nowMs).sort((a, b) => a.dueMs - b.dueMs || d.timers!.findIndex(t => t.id === a.id) - d.timers!.findIndex(t => t.id === b.id))[0];
      if (!due) break; ensure(++firings <= BEHAVIOR_LIMITS.timerFirings, 'Behavior timer work exceeded');
      const timer = d.timers!.find(t => t.id === due.id)!; state.timers = state.timers.filter(t => t.id !== due.id);
      if (timer.repeat) { const dueMs = due.dueMs + timer.delayMs; ensure(Number.isSafeInteger(dueMs), 'Behavior timer clock overflow'); state.timers.push({ id: due.id, dueMs }); }
      dispatch(timer.event, due.dueMs);
    }
    if (event.type === 'event') {
      const declared = d.events.find(e => e.id === event.event); ensure(declared, 'Unknown behavior event');
      object(event.updates ?? {}, [], declared.updates ?? []); object(event.feedback ?? {}, [], declared.updates ?? []);
      for (const [key, value] of Object.entries(event.updates ?? {})) { const signal = d.signals.find(s => s.id === key); ensure(signal?.source === 'input' && accepts(signal, value), 'Invalid external signal update'); state.values[key] = value; }
      for (const [key, sample] of Object.entries(event.feedback ?? {})) { const signal = d.signals.find(s => s.id === key); ensure(signal?.source === 'feedback', 'Invalid feedback update target'); state.feedback[key] = admitFeedback(signal, sample, event.nowMs); }
      dispatch(event.event, event.nowMs, event.suppressCues);
    } else ensure(event.type === 'tick', 'Unknown behavior event type');
    freshen(d, state, event.nowMs); state.nowMs = event.nowMs; return { state, effects, trace };
  } catch (error) { return { state: previous, effects: [], trace: [], error: error instanceof Error ? error.message : 'Invalid behavior event' }; }
}
