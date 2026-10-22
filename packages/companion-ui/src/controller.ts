import type { Control, Value } from '@opensimgear/companion-contracts';
import type { DesignerDocument, DesignerWidget } from '@opensimgear/companion-designer';

export interface PreviewChange { controlId: string; value: Value }
export interface PreviewEvent { sequence: number; atMs: number; reason: string; changes: PreviewChange[] }
export interface PreviewSnapshot { armed: boolean; activePointers: number; pressedWidgetIds?: string[]; values: Record<string, Value>; events: PreviewEvent[]; reason: string }
export interface PointerData { x?: number; y?: number; value?: number; direction?: -1 | 1 }
interface Owner { widgetId: string; controlIds: string[] }
interface Timer { next: number; high: boolean; pulseMs: number; periodMs: number; repeat: boolean; widgetId: string }

export function axisValue(position: number, mode: 'unipolar' | 'bipolar', deadzone: number, inverted = false): number {
  if (!Number.isFinite(position) || !Number.isFinite(deadzone) || deadzone < 0 || deadzone >= 1) throw new Error('Invalid axis position or deadzone');
  let unit = Math.max(0, Math.min(1, position));
  if (inverted) unit = 1 - unit;
  const raw = mode === 'bipolar' ? unit * 2 - 1 : unit;
  const value = Math.abs(raw) <= deadzone ? 0 : Math.sign(raw) * (Math.abs(raw) - deadzone) / (1 - deadzone);
  return Math.round(value * 10000) / 10000;
}
/** Inverse normalization for keyboard increments without applying deadzone/inversion twice. */
export function axisPosition(value: number, mode: 'unipolar' | 'bipolar', deadzone: number, inverted = false): number {
  const raw = value === 0 ? 0 : Math.sign(value) * (deadzone + Math.abs(value) * (1 - deadzone));
  const position = mode === 'bipolar' ? (raw + 1) / 2 : raw;
  return inverted ? 1 - position : position;
}
export function fallbackValue(control: Control): Value { return control.kind === 'button' ? false : control.kind === 'hat' ? -1 : control.fallback; }

/** Recording callbacks only: no transport, output driver, system timer or controller lease. */
export function createPreviewController(initial: DesignerDocument, options: { clock?: () => number; onchange?: (changes: PreviewChange[], reason: string) => void } = {}) {
  const clock = options.clock ?? (() => performance.now());
  let document = initial;
  let armed = false;
  let sequence = 0;
  let lastTick = clock();
  let reason = 'Disarmed';
  const values: Record<string, Value> = {};
  const pointers = new Map<number, Owner>();
  const timers = new Map<string, Timer>();
  const events: PreviewEvent[] = [];
  const subscribers = new Set<(state: PreviewSnapshot) => void>();
  const availability = new Map<string, boolean>();
  const controls = () => new Map(document.ui.controls.map(control => [control.id, control]));
  const widget = (id: string): DesignerWidget | undefined => {
    const item = document.presentation.widgets.find(item => item.id === id);
    if (item?.kind !== 'component') return item;
    const definition = document.presentation.definitions?.find(d => d.id === item.config.definitionId && d.version === item.config.definitionVersion);
    const interaction = definition?.interaction;
    if (interaction?.kind === 'momentary' || interaction?.kind === 'toggle') return { ...item, kind: 'button', config: { buttonMode: interaction.kind, pulseMs: 0, repeatMs: 0 } };
    if (interaction?.kind === 'selector') return { ...item, kind: 'selector', config: { options: interaction.options } };
    return undefined;
  };
  function snapshot(): PreviewSnapshot { return { armed, activePointers: pointers.size, pressedWidgetIds: [...new Set([...pointers.values()].map(p => p.widgetId))], values: { ...values }, events: events.map(event => ({ ...event, changes: event.changes.map(change => ({ ...change })) })), reason }; }
  function notify() { const state = snapshot(); for (const callback of subscribers) callback(state); }
  function write(changes: PreviewChange[], cause: string, atMs = clock()) {
    const changed = changes.filter(change => values[change.controlId] !== change.value);
    if (!changed.length) return;
    for (const change of changed) values[change.controlId] = change.value;
    events.push({ sequence: ++sequence, atMs, reason: cause, changes: changed.map(change => ({ ...change })) });
    if (events.length > 64) events.splice(0, events.length - 64);
    options.onchange?.(changed.map(change => ({ ...change })), cause);
  }
  function neutral(ids: string[], cause: string) {
    const byId = controls();
    write(ids.flatMap(controlId => { const control = byId.get(controlId); return control ? [{ controlId, value: fallbackValue(control) }] : []; }), cause);
  }
  function disarm(cause = 'Preview stopped') {
    neutral(document.ui.controls.map(control => control.id), cause);
    pointers.clear(); timers.clear(); armed = false; reason = cause; notify(); return snapshot();
  }
  function arm() { disarm('New preview'); armed = true; lastTick = clock(); reason = 'Local preview'; notify(); return snapshot(); }
  function subscribe(callback: (state: PreviewSnapshot) => void) { subscribers.add(callback); callback(snapshot()); return () => { subscribers.delete(callback); }; }
  function setDocument(next: DesignerDocument) { disarm('Document changed'); availability.clear(); for (const key of Object.keys(values)) delete values[key]; document = next; initialize(); notify(); }
  function initialize() { for (const control of document.ui.controls) values[control.id] = fallbackValue(control); }
  initialize();
  function startPulse(controlId: string, item: DesignerWidget, pulseMs: number, periodMs: number, repeat: boolean) {
    if (!Number.isInteger(pulseMs) || pulseMs <= 0 || (repeat && (!Number.isInteger(periodMs) || periodMs <= pulseMs))) return false;
    if (item.controlIds.some(id => timers.has(id))) { reason = 'Pulse pending; activation rejected'; notify(); return false; }
    write([{ controlId, value: true }], 'Pulse pressed');
    timers.set(controlId, { next: clock() + pulseMs, high: true, pulseMs, periodMs, repeat, widgetId: item.id });
    return true;
  }
  function axisChanges(item: DesignerWidget, data: PointerData): PreviewChange[] {
    if (item.kind !== 'slider' && item.kind !== 'xy') return [];
    const byId = controls();
    return item.controlIds.flatMap((controlId, index) => {
      const control = byId.get(controlId);
      if (control?.kind !== 'axis') return [];
      const position = item.kind === 'slider' ? (item.config.orientation === 'vertical' ? data.y : data.x) : index === 0 ? data.x : data.y;
      if (position === undefined || !Number.isFinite(position)) return [];
      const inverted = item.kind === 'slider' ? item.config.inverted : index === 0 ? item.config.invertX : item.config.invertY;
      return [{ controlId, value: axisValue(position, control.axisMode, item.config.deadzone, inverted) }];
    });
  }
  function begin(pointerId: number, widgetId: string, data: PointerData = {}): boolean {
    const item = widget(widgetId);
    if (!armed || !item || availability.get(widgetId) === false || !Number.isSafeInteger(pointerId) || pointers.has(pointerId) || pointers.size >= 32 || !item.controlIds.length) return false;
    if ([...pointers.values()].some(owner => owner.controlIds.some(id => item.controlIds.includes(id)))) return false;
    const byId = controls();
    if (item.controlIds.some(id => !byId.has(id))) return false;
    const id = item.controlIds[0];
    let accepted = true;
    switch (item.kind) {
      case 'button': {
        const config = item.config;
        if (config.buttonMode === 'toggle') write([{ controlId: id, value: !values[id] }], 'Toggle changed');
        else if (config.buttonMode === 'pulse' || config.buttonMode === 'repeat') accepted = startPulse(id, item, config.pulseMs, config.repeatMs, config.buttonMode === 'repeat');
        else write([{ controlId: id, value: true }], 'Button pressed');
        break;
      }
      case 'encoder': {
        const output = item.controlIds[data.direction === -1 ? 1 : 0];
        if (!output) { accepted = false; break; }
        accepted = startPulse(output, item, item.config.pulseMs, item.config.repeatMs, item.config.repeatMs > 0);
        break;
      }
      case 'selector': {
        if (!item.config.options.some(option => option.value === data.value)) { accepted = false; break; }
        write([{ controlId: id, value: data.value! }], 'Selection changed'); break;
      }
      case 'hat': {
        if (data.value === undefined || !Number.isInteger(data.value) || data.value < 0 || data.value > 7) { accepted = false; break; }
        write([{ controlId: id, value: data.value }], 'Hat pressed'); break;
      }
      case 'slider': case 'xy': {
        const changes = axisChanges(item, data);
        if (changes.length !== item.controlIds.length) { accepted = false; break; }
        write(changes, 'Axis changed'); break;
      }
      default: accepted = false;
    }
    if (!accepted) return false;
    pointers.set(pointerId, { widgetId, controlIds: [...item.controlIds] }); reason = 'Local preview'; notify(); return true;
  }
  function move(pointerId: number, data: PointerData): boolean {
    const owner = pointers.get(pointerId); const item = owner ? widget(owner.widgetId) : undefined;
    if (!armed || !item || (item.kind !== 'slider' && item.kind !== 'xy')) return false;
    const changes = axisChanges(item, data);
    if (changes.length !== item.controlIds.length) return false;
    write(changes, 'Axis moved'); notify(); return true;
  }
  function release(pointerId: number, cause = 'Pointer released', cancelled = false): boolean {
    const owner = pointers.get(pointerId); if (!owner) return false;
    pointers.delete(pointerId); const item = widget(owner.widgetId);
    const retained = !cancelled && item && (item.kind === 'selector' || (item.kind === 'button' && (item.config.buttonMode === 'toggle' || item.config.buttonMode === 'pulse')) || ((item.kind === 'slider' || item.kind === 'xy') && item.config.centering === 'latched') || (item.kind === 'encoder' && item.config.repeatMs === 0));
    if (!retained) {
      for (const id of owner.controlIds) timers.delete(id);
      neutral(owner.controlIds, cause);
    }
    reason = cause; notify(); return true;
  }
  function cancelWidget(widgetId: string, cause = 'Control lost focus') {
    for (const [pointerId, owner] of [...pointers]) if (owner.widgetId === widgetId) release(pointerId, cause, true);
    const active = [...timers].filter(([, timer]) => timer.widgetId === widgetId).map(([id]) => id);
    for (const id of active) timers.delete(id);
    neutral(active, cause); reason = cause; notify();
  }
  function setWidgetEnabled(widgetId: string, enabled: boolean) {
    if ((availability.get(widgetId) ?? true) === enabled) return;
    availability.set(widgetId, enabled);
    if (!enabled) {
      cancelWidget(widgetId, 'Widget unavailable');
      neutral(widget(widgetId)?.controlIds ?? [], 'Widget unavailable');
      notify();
    }
  }
  function tick(now = clock()) {
    if (!Number.isFinite(now) || now < lastTick) return;
    if (armed && timers.size && now - lastTick > 1000) { disarm('Preview timing gap'); lastTick = now; return; }
    lastTick = now;
    let processed = 0;
    while (armed) {
      const due = [...timers.entries()].filter(([, timer]) => timer.next <= now).sort((a, b) => a[1].next - b[1].next)[0];
      if (!due) break;
      if (++processed > 256) { disarm('Preview timer budget exceeded'); break; }
      const [id, timer] = due;
      write([{ controlId: id, value: !timer.high }], timer.high ? 'Pulse released' : 'Repeat pressed', timer.next);
      if (timer.high && !timer.repeat) timers.delete(id);
      else { timer.next += timer.high ? timer.periodMs - timer.pulseMs : timer.pulseMs; timer.high = !timer.high; }
    }
    notify();
  }
  return { arm, disarm, snapshot, subscribe, setDocument, begin, move, release, cancelWidget, setWidgetEnabled, tick };
}
export type PreviewController = ReturnType<typeof createPreviewController>;
