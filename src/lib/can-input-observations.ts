import { SharedObservations, type ObservationState } from './streams/shared.ts';
import { visibleObservation } from './visible-observation.ts';

export type CanInputRoute = Readonly<{
  epoch: number; node: number; sourceNode: number; boot: number; lease: number; configurationGeneration: number;
}>;
export type CanPanelPreview = Readonly<{
  route: CanInputRoute; held: readonly boolean[]; axes: readonly number[]; axisAvailable: readonly boolean[];
}>;

const observations = new SharedObservations();
const record = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === 'object' && !Array.isArray(value);
const uint = (value: unknown, max: number, min = 0): value is number =>
  typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max;

export function canInputTopic(gatewayId: string, moduleId: string): string | undefined {
  const gateway = /^stm32:([0-9a-f]{24})$/.exec(gatewayId);
  if (gateway && /^[0-9a-f]{32}$/.test(moduleId)) return `can.inputs.${gateway[1]}.${moduleId}`;
}

export function canPanelPreview(state: ObservationState, topic: string): CanPanelPreview | undefined {
  if (state.status !== 'ready' || state.frame?.topic !== topic || state.frame.source !== 'can-inputs') return;
  const value = state.frame.payload;
  if (!record(value) || value.status !== 'ready' || !record(value.route)) return;
  const route = value.route;
  if (!uint(route.node, 32, 1) || !uint(route.sourceNode, 127)
    || !uint(route.epoch, 0xffffffff, 1) || !uint(route.boot, 0xffffffff, 1)
    || !uint(route.lease, 0xffffffff, 1) || !uint(route.configurationGeneration, 0xffffffff, 1)
    || !Array.isArray(value.held) || value.held.length > 128 || !value.held.every(item => typeof item === 'boolean')
    || !Array.isArray(value.axes) || value.axes.length !== 8 || !value.axes.every(item => uint(item, 65535))
    || !Array.isArray(value.axisAvailable) || value.axisAvailable.length !== 8 || !value.axisAvailable.every(item => typeof item === 'boolean')) return;
  return { route: route as CanInputRoute, held: value.held, axes: value.axes, axisAvailable: value.axisAvailable };
}

export function observeCanPanel(topic: string, listener: (preview?: CanPanelPreview) => void): () => void {
  const reader = visibleObservation(observations, topic, state => listener(canPanelPreview(state, topic)), () => listener());
  reader.setVisible(true);
  return () => reader.close();
}
