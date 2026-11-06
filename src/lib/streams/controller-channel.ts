import type { DeliveryEnvelope } from './protocol.ts';

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** Expand transport-only controller samples before exposing canonical frames. */
export function controllerChannelDecoder() {
  let baseline: DeliveryEnvelope['frame'] | undefined;
  return (envelope: DeliveryEnvelope): DeliveryEnvelope => {
    const frame = envelope.frame;
    const payload = frame.payload;
    if (frame.source !== 'controller-input' || frame.kind !== 'snapshot' || !record(payload)) {
      baseline = undefined;
      return envelope;
    }
    if (!Object.hasOwn(payload, 'controllerSamples')) {
      baseline = structuredClone(frame);
      return envelope;
    }
    const previous = baseline?.payload;
    const samples = payload.controllerSamples;
    if (baseline?.generation !== frame.generation || baseline.session !== frame.session
      || !record(previous) || !Array.isArray(previous.controls) || !Array.isArray(samples)
      || samples.length !== previous.controls.length || samples.length > 256
      || Object.keys(payload).length !== 4 || !record(payload.source)
      || typeof payload.cursor !== 'string' || typeof payload.capturedUs !== 'string') {
      throw new Error('Invalid compact controller snapshot');
    }
    const {controls, ...metadata} = previous;
    const restored = structuredClone(metadata);
    restored.source = payload.source;
    restored.cursor = payload.cursor;
    restored.capturedUs = payload.capturedUs;
    restored.controls = controls.map((row, index) => {
      const sample = samples[index];
      if (!record(row) || (sample !== null && (!Array.isArray(sample) || sample.length !== 2
        || !Number.isInteger(sample[0]) || (sample[1] !== null && !Number.isInteger(sample[1]))))) {
        throw new Error('Invalid compact controller sample');
      }
      return { capability: structuredClone(row.capability),
        value: sample === null ? null : { native: sample[0], normalized: sample[1] } };
    });
    return { ...envelope, frame: { ...frame, payload: restored } };
  };
}
