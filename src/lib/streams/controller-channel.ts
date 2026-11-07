import type { DeliveryEnvelope } from './protocol.ts';

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function scalarRecord(value: unknown): value is Record<string, unknown> {
  return record(value) && Object.getPrototypeOf(value) === Object.prototype
    && Object.values(value).every(field => field === null || typeof field !== 'object');
}

/** Expand transport-only controller samples before exposing canonical frames. */
export function controllerChannelDecoder() {
  let baseline: {
    generation: string;
    session: string;
    metadata: Record<string, unknown>;
    capabilityCount: number;
    copyCapabilities: () => unknown[];
  } | undefined;
  return (envelope: DeliveryEnvelope): DeliveryEnvelope => {
    const frame = envelope.frame;
    const payload = frame.payload;
    if (frame.source !== 'controller-input' || frame.kind !== 'snapshot' || !record(payload)) {
      baseline = undefined;
      return envelope;
    }
    if (!Object.hasOwn(payload, 'controllerSamples')) {
      const {controls, ...metadata} = payload;
      baseline = undefined;
      if (Array.isArray(controls) && controls.every(record)) {
        const capabilities = structuredClone(controls.map(row => row.capability));
        // Native descriptors contain scalar fields. Copy their records without
        // repeatedly serializing the entire array through structuredClone.
        const copyCapabilities = capabilities.every(scalarRecord)
          ? () => capabilities.map(capability => ({...capability}))
          : () => structuredClone(capabilities);
        baseline = { generation: frame.generation, session: frame.session,
          metadata: structuredClone(metadata), capabilityCount: capabilities.length, copyCapabilities };
      }
      return envelope;
    }
    const samples = payload.controllerSamples;
    if (baseline?.generation !== frame.generation || baseline.session !== frame.session
      || !Array.isArray(samples)
      || samples.length !== baseline.capabilityCount || samples.length > 256
      || Object.keys(payload).length !== 4 || !record(payload.source)
      || typeof payload.cursor !== 'string' || typeof payload.capturedUs !== 'string') {
      throw new Error('Invalid compact controller snapshot');
    }
    const restored = structuredClone(baseline.metadata);
    restored.source = payload.source;
    restored.cursor = payload.cursor;
    restored.capturedUs = payload.capturedUs;
    restored.controls = baseline.copyCapabilities().map((capability, index) => {
      const sample = samples[index];
      if (sample !== null && (!Array.isArray(sample) || sample.length !== 2
        || !Number.isInteger(sample[0]) || (sample[1] !== null && !Number.isInteger(sample[1])))) {
        throw new Error('Invalid compact controller sample');
      }
      return { capability,
        value: sample === null ? null : { native: sample[0], normalized: sample[1] } };
    });
    return { ...envelope, frame: { ...frame, payload: restored } };
  };
}
