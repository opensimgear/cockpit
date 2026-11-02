export interface WireFrame {
  kind: 'snapshot' | 'change' | 'loss';
  topic: string;
  source: string;
  session: string;
  generation: string;
  revision: string;
  sequence?: string;
  capturedUs: string;
  clockId: string;
  continuity: 'baseline' | 'continuous' | 'lost';
  payload?: unknown;
  reason?: string;
}

export interface DeliveryEnvelope {
  subscriptionId: string;
  delivery: string;
  frame: WireFrame;
}

export const MAX_SNAPSHOT_BYTES = 1024 * 1024;
export const MAX_CHANGE_BYTES = 64 * 1024;
export const MAX_LINE_BYTES = MAX_SNAPSHOT_BYTES + 4096;
const encoder = new TextEncoder();

export function uint64(value: unknown): value is string {
  return typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value)
    && BigInt(value) <= 18446744073709551615n;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function text(value: unknown, max = 256): value is string {
  return typeof value === 'string' && value.length > 0 && encoder.encode(value).length <= max;
}

/** Validate the entire boundary before exposing any state to consumers. */
export function parseDelivery(value: unknown, topic: string): DeliveryEnvelope {
  if (!record(value) || !text(value.subscriptionId) || !uint64(value.delivery) || !record(value.frame)) {
    throw new Error('Invalid observation delivery envelope');
  }
  const frame = value.frame;
  if (typeof frame.kind !== 'string' || !['snapshot', 'change', 'loss'].includes(frame.kind)
      || typeof frame.continuity !== 'string' || !['baseline', 'continuous', 'lost'].includes(frame.continuity)
      || frame.topic !== topic || !text(frame.topic) || !text(frame.source)
      || !text(frame.session) || !text(frame.clockId)
      || !uint64(frame.generation) || !uint64(frame.revision) || !uint64(frame.capturedUs)
      || (frame.sequence !== undefined && !uint64(frame.sequence))
      || (frame.reason !== undefined && !text(frame.reason, 1024))) {
    throw new Error('Invalid observation frame metadata');
  }
  if ((frame.kind === 'snapshot' && frame.continuity !== 'baseline')
      || (frame.kind === 'change' && frame.continuity !== 'continuous')
      || (frame.kind === 'loss' && frame.continuity !== 'lost')) {
    throw new Error('Invalid observation continuity');
  }
  const encoded = JSON.stringify(value, (_key, item) => {
    if (typeof item === 'number' && !Number.isFinite(item)) {
      throw new Error('Observation payload contains a nonfinite number');
    }
    return item;
  });
  const limit = frame.kind === 'snapshot' ? MAX_SNAPSHOT_BYTES : MAX_CHANGE_BYTES;
  if (encoder.encode(encoded).length > MAX_LINE_BYTES
      || encoder.encode(JSON.stringify(frame)).length > limit
      || encoder.encode(JSON.stringify({ ...frame, payload: undefined })).length > 8192) {
    throw new Error('Observation frame exceeds byte budget');
  }
  return structuredClone(value) as unknown as DeliveryEnvelope;
}

export interface ObservationObserver {
  frame: (envelope: unknown, acknowledge: () => Promise<void>) => void;
  error: (message: string) => void;
}
export interface ObservationTransport {
  open: (topic: string, observer: ObservationObserver, signal: AbortSignal) => void;
}
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
