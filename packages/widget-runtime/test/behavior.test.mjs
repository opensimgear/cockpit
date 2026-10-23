import test from 'node:test';
import assert from 'node:assert/strict';
import {
  BEHAVIOR_LIMITS, validateBehaviorDefinition, createBehaviorState, reduceBehavior, evaluateExpression,
} from '../src/behavior.ts';

const literal = value => ({ literal: value });
const ref = value => ({ ref: value });
const not = value => ({ op: 'not', value });
const equal = (left, right) => ({ op: 'eq', left, right });
const feedback = value => ({ status: 'fresh', value, provenance: 'game-confirmed', sourceKind: 'simulator', sourceAgeMs: 0 });
function toggle() {
  return {
    version: 1,
    signals: [
      { id: 'requested', source: 'local', type: 'boolean', initial: false },
      { id: 'pressed', source: 'input', type: 'boolean', initial: false },
      { id: 'available', source: 'input', type: 'boolean', initial: true },
      { id: 'confirmed', source: 'feedback', type: 'boolean', initial: false, freshnessMs: 1000, qualitySignal: 'quality', requireGameConfirmed: true },
      { id: 'quality', source: 'input', type: 'enum', values: ['fresh', 'unknown', 'stale', 'disconnected'], initial: 'unknown' },
    ],
    outputs: [{ id: 'control', type: 'boolean', neutral: false }],
    events: [{ id: 'activate' }, { id: 'press', updates: ['pressed'] }, { id: 'availability', updates: ['available'] }, { id: 'feedback', updates: ['confirmed'] }, { id: 'expire' }],
    regions: [
      { id: 'intent', initial: 'off', states: ['off', 'on'], transitions: [
        { id: 'turn-on', from: 'off', to: 'on', event: 'activate', guard: ref('available'), actions: [{ type: 'set', signal: 'requested', value: literal(true) }, { type: 'emit', port: 'control', value: literal(true) }, { type: 'cue', cue: 'click' }] },
        { id: 'turn-off', from: 'on', to: 'off', event: 'activate', guard: ref('available'), actions: [{ type: 'set', signal: 'requested', value: literal(false) }, { type: 'emit', port: 'control', value: literal(false) }, { type: 'cue', cue: 'click' }] },
      ] },
      { id: 'touch', initial: 'released', states: ['released', 'held'], transitions: [
        { id: 'hold', from: '*', to: 'held', event: 'press', priority: 1, guard: ref('pressed') },
        { id: 'release', from: '*', to: 'released', event: 'press', priority: 0, guard: not(ref('pressed')) },
      ] },
    ],
  };
}
function send(d, state, event, patch = {}) { const result = reduceBehavior(d, state, { type: 'event', event, nowMs: state.nowMs + 1, ...patch }); assert.equal(result.error, undefined); return result; }

test('Toggle uses generic independent regions: requested changes immediately without confirmation', () => {
  const definition = toggle(), initial = createBehaviorState(definition, 100);
  const confirmedOff = send(definition, initial, 'feedback', { feedback: { confirmed: feedback(false) } });
  const on = send(definition, confirmedOff.state, 'activate');
  assert.equal(on.state.values.requested, true);
  assert.equal(on.state.values.confirmed, false);
  assert.equal(on.state.values.quality, 'fresh');
  assert.deepEqual(on.effects, [{ type: 'emit', port: 'control', value: true }, { type: 'cue', cue: 'click' }]);
  assert.equal(on.state.regions.intent, 'on');
  const held = send(definition, on.state, 'press', { updates: { pressed: true } });
  assert.equal(held.state.regions.touch, 'held');
  assert.equal(held.state.regions.intent, 'on');
  const confirmedOn = send(definition, held.state, 'feedback', { feedback: { confirmed: feedback(true) } });
  const off = send(definition, confirmedOn.state, 'activate');
  assert.equal(off.state.values.requested, false);
  assert.equal(off.state.values.confirmed, true);
  assert.deepEqual(initial.values, { requested: false, pressed: false, available: true, confirmed: false, quality: 'unknown' });
});

test('feedback freshness accounts for source age, rejects false confirmation, and resets independently', () => {
  const d = toggle(); let state = send(d, createBehaviorState(d), 'activate').state;
  state = send(d, state, 'feedback', { feedback: { confirmed: { ...feedback(true), sourceAgeMs: 900 } }, nowMs: 20 }).state;
  let result = reduceBehavior(d, state, { type: 'tick', nowMs: 119 }); assert.equal(result.state.values.quality, 'fresh');
  result = reduceBehavior(d, result.state, { type: 'tick', nowMs: 120 });
  assert.equal(result.state.values.quality, 'stale'); assert.equal(result.state.values.confirmed, false); assert.equal(result.state.values.requested, true);
  state = send(d, result.state, 'feedback', { feedback: { confirmed: { status: 'disconnected' } } }).state;
  assert.equal(state.values.quality, 'disconnected'); assert.equal(state.values.requested, true);
  state = send(d, state, 'feedback', { feedback: { confirmed: { ...feedback(true), provenance: 'host-accepted', sourceKind: 'host' } } }).state;
  assert.equal(state.values.quality, 'unknown'); assert.equal(state.values.confirmed, false);
  const reset = reduceBehavior(d, state, { type: 'reset', nowMs: state.nowMs + 1 });
  assert.deepEqual(reset.effects, [{ type: 'emit', port: 'control', value: false }]); assert.deepEqual(reset.trace, []);
  assert.equal(reset.state.values.requested, false); assert.equal(reset.state.values.quality, 'unknown'); assert.equal(reset.state.regions.intent, 'off');
  assert.deepEqual(reset.state.feedback, {});
});

test('availability guards suppress input, restored snapshots suppress cues, and guards/actions read one snapshot', () => {
  const d = toggle();
  let state = send(d, createBehaviorState(d), 'availability', { updates: { available: false } }).state;
  const ignored = send(d, state, 'activate'); assert.deepEqual(ignored.effects, []); assert.equal(ignored.state.values.requested, false);
  state = send(d, state, 'availability', { updates: { available: true } }).state;
  const restored = send(d, state, 'activate', { suppressCues: true }); assert.deepEqual(restored.effects, [{ type: 'emit', port: 'control', value: true }]);
  d.regions[0].transitions[0].actions = [{ type: 'set', signal: 'requested', value: not(ref('requested')) }, { type: 'emit', port: 'control', value: ref('requested') }];
  const result = send(d, createBehaviorState(d), 'activate'); assert.equal(result.state.values.requested, true); assert.equal(result.effects[0].value, false);
});

test('orthogonal transitions reject conflicting writes atomically, including prior timer effects', () => {
  const d = toggle(); d.regions.push({ id: 'conflict', initial: 'idle', states: ['idle'], transitions: [{ id: 'other', from: '*', event: 'activate', actions: [{ type: 'set', signal: 'requested', value: literal(false) }] }] });
  const state = createBehaviorState(d); const result = reduceBehavior(d, state, { type: 'event', event: 'activate', nowMs: 1 });
  assert.match(result.error, /Conflicting writes/); assert.equal(result.state, state); assert.deepEqual(result.effects, []); assert.deepEqual(result.trace, []);
  d.regions[1].transitions.push({ id: 'conflicting-emit', from: '*', event: 'activate', actions: [{ type: 'emit', port: 'control', value: literal(false) }] });
  d.regions.pop(); assert.match(reduceBehavior(d, createBehaviorState(d), { type: 'event', event: 'activate', nowMs: 1 }).error, /Conflicting writes/);
  const timed = timers(); timed.events.push({ id: 'conflict' });
  timed.regions[0].transitions.push({ id: 'conflict', from: '*', event: 'conflict', actions: [{ type: 'set', signal: 'count', value: literal(0) }] });
  timed.regions.push({ id: 'other', initial: 'idle', states: ['idle'], transitions: [{ id: 'also', from: '*', event: 'conflict', actions: [{ type: 'set', signal: 'count', value: literal(1) }] }] });
  const armed = send(timed, createBehaviorState(timed), 'start', { nowMs: 0 }).state;
  const conflict = reduceBehavior(timed, armed, { type: 'event', event: 'conflict', nowMs: 10 });
  assert.match(conflict.error, /Conflicting/); assert.equal(conflict.state, armed); assert.deepEqual(conflict.effects, []); assert.deepEqual(conflict.trace, []);
});

test('strict external updates cannot write locals, forge quality, or introduce values outside domains', () => {
  const d = toggle(), state = createBehaviorState(d);
  for (const event of [
    { event: 'missing' }, { event: 'activate', updates: { requested: true } },
    { event: 'press', updates: { pressed: 1 } }, { event: 'press', updates: { quality: 'fresh' } },
    { event: 'feedback', updates: { confirmed: true } }, { event: 'press', feedback: { pressed: feedback(true) } },
    { event: 'feedback', feedback: { confirmed: { status: 'fresh' } } },
    { event: 'feedback', feedback: { confirmed: { ...feedback(true), sourceAgeMs: -1 } } },
    { event: 'feedback', feedback: { confirmed: { ...feedback(true), provenance: 'forged' } } },
    { event: 'feedback', feedback: { confirmed: { ...feedback(true), sourceKind: 'forged' } } },
    { event: 'feedback', feedback: { confirmed: { ...feedback(true), sourceKind: 'host' } } },
    { event: 'feedback', feedback: { confirmed: { status: 'unknown', value: true } } },
    { event: 'feedback', feedback: { confirmed: { status: 'stale', value: 1 } } },
    { event: 'feedback', feedback: { confirmed: { status: 'bad' } } },
  ]) {
    const result = reduceBehavior(d, state, { type: 'event', nowMs: 1, ...event }); assert.ok(result.error, JSON.stringify(event)); assert.equal(result.state, state); assert.deepEqual(result.effects, []);
  }
  const stale = send(d, state, 'feedback', { feedback: { confirmed: { status: 'stale', value: true } } }); assert.equal(stale.state.values.quality, 'stale'); assert.equal(stale.state.values.confirmed, false);
  const unknown = send(d, stale.state, 'feedback', { feedback: { confirmed: { status: 'unknown' } } }); assert.equal(unknown.state.values.quality, 'unknown');
});

function timers() {
  return {
    version: 1,
    signals: [{ id: 'count', source: 'local', type: 'number', min: 0, max: 1000, initial: 0 }],
    outputs: [{ id: 'count', type: 'number', min: 0, max: 1000, neutral: 0 }],
    events: [{ id: 'start' }, { id: 'stop' }, { id: 'first' }, { id: 'second' }],
    timers: [{ id: 'a', event: 'first', delayMs: 10, repeat: true }, { id: 'b', event: 'second', delayMs: 10 }],
    regions: [{ id: 'main', initial: 'idle', states: ['idle'], transitions: [
      { id: 'start', from: '*', event: 'start', actions: [{ type: 'start-timer', timer: 'a' }, { type: 'start-timer', timer: 'b' }] },
      { id: 'first', from: '*', event: 'first', actions: [{ type: 'set', signal: 'count', value: { op: 'clamp', value: { op: 'add', left: ref('count'), right: literal(1) }, min: literal(0), max: literal(1000) } }, { type: 'cue', cue: 'first' }] },
      { id: 'second', from: '*', event: 'second', actions: [{ type: 'emit', port: 'count', value: ref('count') }, { type: 'cue', cue: 'second' }] },
      { id: 'stop', from: '*', event: 'stop', actions: [{ type: 'cancel-timer', timer: 'a' }] },
    ] }],
  };
}

test('timers are explicit, ordered by due time and declaration, repeat boundedly, cancel, and reset', () => {
  const d = timers(); let state = send(d, createBehaviorState(d), 'start', { nowMs: 0 }).state;
  const first = reduceBehavior(d, state, { type: 'tick', nowMs: 10 }); assert.equal(first.error, undefined);
  assert.deepEqual(first.effects, [{ type: 'cue', cue: 'first' }, { type: 'emit', port: 'count', value: 1 }, { type: 'cue', cue: 'second' }]);
  assert.deepEqual(first.trace.map(t => [t.transition, t.atMs]), [['first', 10], ['second', 10]]);
  const second = reduceBehavior(d, first.state, { type: 'tick', nowMs: 35 }); assert.equal(second.state.values.count, 3); assert.deepEqual(second.trace.map(t => t.atMs), [20, 30]);
  const stopped = send(d, second.state, 'stop', { nowMs: 40 }); assert.equal(stopped.state.values.count, 4); assert.deepEqual(stopped.state.timers, []);
  const reset = reduceBehavior(d, state, { type: 'reset', nowMs: 100 }); assert.deepEqual(reset.state.timers, []); assert.equal(reset.state.values.count, 0); assert.deepEqual(reset.effects, [{ type: 'emit', port: 'count', value: 0 }]);
  const exceeded = reduceBehavior(d, state, { type: 'tick', nowMs: 10000 }); assert.match(exceeded.error, /timer work/); assert.equal(exceeded.state, state); assert.deepEqual(exceeded.effects, []);
  const backwards = reduceBehavior(d, second.state, { type: 'tick', nowMs: 34 }); assert.match(backwards.error, /Non-monotonic/);
  const overflowStart = reduceBehavior(d, createBehaviorState(d, Number.MAX_SAFE_INTEGER - 5), { type: 'event', event: 'start', nowMs: Number.MAX_SAFE_INTEGER - 5 }); assert.match(overflowStart.error, /overflow/);
});

test('numeric and enum domains, state expressions, ordered priority and bounded arithmetic are generic', () => {
  const d = timers(); d.signals.push({ id: 'mode', source: 'input', type: 'enum', initial: 'off', values: ['off', 'on'] }); d.events.push({ id: 'mode', updates: ['mode'] });
  d.regions[0].states.push('active');
  d.regions[0].transitions.push(
    { id: 'low', from: '*', event: 'mode', priority: 1, to: 'idle', guard: literal(true) },
    { id: 'high', from: '*', event: 'mode', priority: 2, to: 'active', guard: { op: 'and', values: [equal(ref('mode'), literal('on')), equal({ state: 'main' }, literal('idle'))] } },
  );
  const result = send(d, createBehaviorState(d), 'mode', { updates: { mode: 'on' } }); assert.equal(result.state.regions.main, 'active'); assert.equal(result.trace[0].transition, 'high');
  assert.match(reduceBehavior(d, result.state, { type: 'event', event: 'mode', updates: { mode: 'third' }, nowMs: 2 }).error, /external/);
  const context = { values: { x: 6, flag: true }, regions: { main: 'active' } };
  for (const [op, expected] of [['lt', false], ['lte', true], ['gt', false], ['gte', true], ['eq', true], ['ne', false], ['add', 12], ['sub', 0], ['mul', 36], ['div', 1]]) assert.equal(evaluateExpression({ op, left: ref('x'), right: literal(6) }, context), expected);
  assert.equal(evaluateExpression({ op: 'or', values: [literal(false), ref('flag')] }, context), true);
  assert.equal(evaluateExpression({ op: 'if', condition: literal(false), then: literal('a'), else: literal('b') }, context), 'b');
  assert.equal(evaluateExpression({ op: 'clamp', value: literal(7), min: literal(0), max: literal(5) }, context), 5);
  assert.throws(() => evaluateExpression({ ref: 'missing' }, context), /Missing/);
  assert.throws(() => evaluateExpression({ state: 'missing' }, context), /Missing/);
  assert.throws(() => evaluateExpression({ op: 'div', left: literal(1), right: literal(0) }, context), /zero/);
  assert.throws(() => evaluateExpression({ op: 'not', value: literal(1) }, context), /Boolean/);
  assert.throws(() => evaluateExpression({ op: 'if', condition: literal(1), then: literal(1), else: literal(2) }, context), /Boolean/);
  assert.throws(() => evaluateExpression({ op: 'and', values: [literal(1)] }, context), /Boolean/);
  assert.throws(() => evaluateExpression({ op: 'clamp', value: literal(1), min: literal(2), max: literal(0) }, context), /clamp/);
  assert.throws(() => evaluateExpression({ op: 'eq', left: literal(1), right: literal(true) }, context), /types/);
  assert.throws(() => evaluateExpression({ op: 'lt', left: literal(true), right: literal(false) }, context), /Numeric/);
  assert.throws(() => evaluateExpression({ op: 'not-supported', left: literal(1), right: literal(2) }, context), /Unsupported/);
  assert.throws(() => evaluateExpression({ op: 'and', values: Array.from({ length: 16 }, () => not(literal(true))) }, context), /work/);
  assert.throws(() => evaluateExpression(Array.from({ length: 8 }).reduce(value => not(value), literal(true)), context), /work/);
});

test('strict imports detach definitions and reject invalid domains, targets, priorities, AST types and bounds', () => {
  const source = toggle(), validated = validateBehaviorDefinition(source); assert.equal(validated.ok, true); source.signals[0].initial = true; assert.equal(validated.value.signals[0].initial, false);
  const changes = [
    d => d.version = 2, d => d.extra = true, d => d.signals[0].id = 'bad/id', d => d.signals.push(d.signals[0]),
    d => d.signals[0].initial = 1, d => d.signals[0].source = 'script', d => d.signals[0].freshnessMs = 100,
    d => d.signals[3].freshnessMs = 0, d => d.signals[3].requireGameConfirmed = 1, d => d.signals[3].qualitySignal = 'requested',
    d => d.signals[4].values.push('fresh'), d => d.signals[4].initial = 'bad', d => d.signals[4].values = [],
    d => d.events[0].updates = ['requested'], d => d.events[0].updates = ['quality'], d => d.events[0].updates = ['available', 'available'],
    d => d.events[0].updates = ['missing'], d => d.events.push(d.events[0]), d => d.outputs[0].neutral = 1,
    d => d.regions[0].initial = 'absent', d => d.regions[0].states.push('off'),
    d => d.regions[0].transitions[0].from = 'absent', d => d.regions[0].transitions[0].to = 'absent',
    d => d.regions[0].transitions[0].event = 'absent', d => d.regions[0].transitions[0].priority = 1.5,
    d => d.regions[0].transitions.push({ ...d.regions[0].transitions[0], id: 'duplicate' }),
    d => d.regions[0].transitions.push({ ...d.regions[0].transitions[0], id: 'wildcard', from: '*' }),
    d => d.regions[0].transitions[0].guard = literal('wrong'),
    d => d.regions[0].transitions[0].guard = { ref: 'absent' },
    d => d.regions[0].transitions[0].guard = { state: 'absent' },
    d => d.regions[0].transitions[0].guard = { op: 'eval', left: literal(1), right: literal(1) },
    d => d.regions[0].transitions[0].guard = { op: 'eq', left: literal(1), right: literal(true) },
    d => d.regions[0].transitions[0].guard = { op: 'gt', left: literal(true), right: literal(false) },
    d => d.regions[0].transitions[0].guard = { op: 'not', value: literal(1) },
    d => d.regions[0].transitions[0].guard = { op: 'or', values: [] },
    d => d.regions[0].transitions[0].actions[0].signal = 'pressed',
    d => d.regions[0].transitions[0].actions[1].port = 'absent',
    d => d.regions[0].transitions[0].actions[0].value = literal(1),
    d => d.regions[0].transitions[0].actions.push(d.regions[0].transitions[0].actions[0]),
    d => d.regions[0].transitions[0].actions.push({ type: 'cue', cue: 'bad/id' }),
    d => d.regions[0].transitions[0].actions.push({ type: 'start-timer', timer: 'absent' }),
    d => d.regions[0].transitions[0].guard = Array.from({ length: 9 }).reduce(value => ({ op: 'not', value }), literal(true)),
    d => d.regions[0].transitions[0].guard = { op: 'and', values: Array.from({ length: 16 }, () => ({ op: 'not', value: literal(true) })) },
    d => d.signals[0].initial = null,
  ];
  for (const [index, change] of changes.entries()) { const d = toggle(); change(d); assert.equal(validateBehaviorDefinition(d).ok, false, `invalid definition ${index}`); }
  assert.equal(validateBehaviorDefinition(null).ok, false);
  const cyclic = toggle(); cyclic.loop = cyclic; assert.equal(validateBehaviorDefinition(cyclic).ok, false);
  assert.match(validateBehaviorDefinition({ huge: 'a'.repeat(BEHAVIOR_LIMITS.bytes) }).error, /budget/);
  assert.throws(() => createBehaviorState({ version: 2 }), /behavior/);
  assert.throws(() => createBehaviorState(toggle(), -1), /bounds/);
});

test('numeric inference rejects overflow and out-of-range assignments, supports bounded operators and enum outputs', () => {
  const d = timers();
  const action = expression => { const next = structuredClone(d); next.regions[0].transitions[1].actions[0].value = expression; return validateBehaviorDefinition(next); };
  for (const op of ['add', 'sub', 'mul', 'div']) {
    const expression = { op, left: literal(6), right: literal(2) }; assert.equal(action(expression).ok, true, op);
  }
  for (const op of ['lt', 'lte', 'gt', 'gte', 'eq', 'ne']) { const next = toggle(); next.regions[0].transitions[0].guard = { op, left: literal(6), right: literal(2) }; assert.equal(validateBehaviorDefinition(next).ok, true, op); }
  assert.equal(action({ op: 'div', left: literal(1), right: literal(0) }).ok, false);
  assert.equal(action({ op: 'mul', left: literal(1e9), right: literal(2) }).ok, false);
  assert.equal(action({ op: 'add', left: ref('count'), right: literal(1) }).ok, false);
  assert.equal(action({ op: 'if', condition: literal(true), then: literal(1), else: literal(2) }).ok, true);
  assert.equal(action({ op: 'if', condition: literal(true), then: literal(1), else: literal(false) }).ok, false);
  assert.equal(action({ op: 'if', condition: literal(1), then: literal(1), else: literal(2) }).ok, false);
  assert.equal(action({ op: 'clamp', value: ref('count'), min: literal(2), max: literal(1) }).ok, false);
  d.signals.push({ id: 'choice', source: 'local', type: 'enum', values: ['a', 'b'], initial: 'a' });
  d.outputs.push({ id: 'choice', type: 'enum', values: ['a', 'b'], neutral: 'a' });
  d.regions[0].transitions[0].actions.push({ type: 'set', signal: 'choice', value: { op: 'if', condition: literal(true), then: literal('b'), else: literal('a') } }, { type: 'emit', port: 'choice', value: ref('choice') });
  assert.equal(validateBehaviorDefinition(d).ok, true); const result = send(d, createBehaviorState(d), 'start'); assert.equal(result.state.values.choice, 'b'); assert.equal(result.effects[0].value, 'a');
  const invalids = [
    next => next.signals[0].min = 1000,
    next => next.signals[0].max = Infinity,
    next => next.timers[0].delayMs = 0,
    next => next.timers[0].repeat = 'yes',
    next => next.timers[0].event = 'absent',
    next => next.regions[0].transitions[0].actions.push({ type: 'cancel-timer', timer: 'a' }),
  ];
  for (const change of invalids) { const next = timers(); change(next); assert.equal(validateBehaviorDefinition(next).ok, false); }
});

test('untrusted snapshots and event clocks fail atomically', () => {
  const d = toggle(), state = createBehaviorState(d);
  for (const nowMs of [NaN, Infinity, -1, 0.1]) assert.ok(reduceBehavior(d, state, { type: 'tick', nowMs }).error);
  for (const patch of [{ values: {} }, { regions: {} }, { timers: [{ id: 'absent', dueMs: 2 }] }, { values: { ...state.values, requested: 1 } }]) {
    const previous = { ...state, ...patch }; const result = reduceBehavior(d, previous, { type: 'tick', nowMs: 2 }); assert.ok(result.error); assert.equal(result.state, previous);
  }
  for (const sample of [
    { ...feedback(true), receivedAtMs: 3 },
    { ...feedback(true), receivedAtMs: 0, provenance: 'local', sourceKind: 'host' },
    { ...feedback(true), receivedAtMs: 0, value: 1 },
  ]) {
    const previous = { ...state, feedback: { confirmed: sample } }; assert.ok(reduceBehavior(d, previous, { type: 'tick', nowMs: 2 }).error);
  }
  assert.ok(reduceBehavior(d, { ...state, feedback: { absent: feedback(true) } }, { type: 'tick', nowMs: 2 }).error);
  assert.match(reduceBehavior(d, state, { type: 'event', event: 'activate', nowMs: 1, suppressCues: 'yes' }).error, /suppression/);
  assert.ok(reduceBehavior(d, state, { type: 'tick', nowMs: 1, extra: true }).error);
  assert.match(reduceBehavior(d, state, { type: 'invalid', nowMs: 1 }).error, /Unknown/);
});
