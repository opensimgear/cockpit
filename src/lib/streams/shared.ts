import { errorMessage, parseDelivery, type DeliveryEnvelope, type ObservationTransport, type WireFrame } from './protocol.ts';
import { createObservationTransport } from './transport.ts';

export interface ObservationState {
  status: 'connecting' | 'ready' | 'lost' | 'error';
  frame?: WireFrame;
  error?: string;
}
export type ObservationListener = (state: ObservationState) => void;
interface Entry {
  controller: AbortController;
  listeners: Set<ObservationListener>;
  state: ObservationState;
  token?: string;
  delivery?: bigint;
  baseline?: WireFrame;
  boundary?: WireFrame;
  started?: boolean;
}

/** Exact-topic sharing. Loss withdraws data until a fresh baseline is delivered. */
export class SharedObservations {
  private readonly entries = new Map<string, Entry>();
  private closed = false;

  private readonly transport: ObservationTransport;

  constructor(transport: ObservationTransport = createObservationTransport()) {
    this.transport = transport;
  }

  subscribe(topic: string, listener: ObservationListener): () => void {
    if (this.closed) throw new Error('Observations are disposed');
    let entry = this.entries.get(topic);
    if (!entry) {
      entry = { controller: new AbortController(), listeners: new Set(), state: { status: 'connecting' } };
      this.entries.set(topic, entry);
    }
    // Separate registrations remain independently disposable even for the same callback.
    const registration: ObservationListener = (state) => listener(state);
    entry.listeners.add(registration);
    this.notify(registration, entry.state);
    const current = entry;
    if (!entry.started && this.active(topic, entry)) {
      entry.started = true;
      try {
        this.transport.open(topic, {
          frame: (raw, acknowledge) => {
            if (!this.active(topic, current)) return;
            let envelope: DeliveryEnvelope;
            try { envelope = parseDelivery(raw, topic); }
            catch (error) { this.fail(topic, current, errorMessage(error)); return; }
            try { this.accept(current, envelope); }
            catch (error) { this.fail(topic, current, errorMessage(error)); return; }
            if (this.active(topic, current)) {
              // All cache/listener work is synchronous and precedes acknowledgment.
              void acknowledge().catch((error) => this.fail(topic, current, errorMessage(error)));
            }
          },
          error: (message) => this.fail(topic, current, message),
        }, entry.controller.signal);
      } catch (error) { this.fail(topic, entry, errorMessage(error)); }
    }
    let subscribed = true;
    return () => {
      if (!subscribed) return;
      subscribed = false;
      current.listeners.delete(registration);
      if (current.listeners.size === 0 && this.entries.get(topic) === current) {
        this.entries.delete(topic);
        current.controller.abort();
      }
    };
  }

  close(): void {
    this.closed = true;
    for (const entry of this.entries.values()) {
      entry.listeners.clear();
      entry.controller.abort();
    }
    this.entries.clear();
  }

  dispose(): void { this.close(); }

  private active(topic: string, entry: Entry): boolean {
    return this.entries.get(topic) === entry && !entry.controller.signal.aborted;
  }

  private notify(listener: ObservationListener, state: ObservationState): void {
    try { listener(structuredClone(state)); } catch { /* One consumer cannot stall other observers or ACK. */ }
  }

  private publish(entry: Entry, state: ObservationState): void {
    entry.state = state;
    for (const listener of [...entry.listeners]) {
      if (!entry.controller.signal.aborted && entry.listeners.has(listener)) this.notify(listener, state);
    }
  }

  private fail(topic: string, entry: Entry, message: string): void {
    if (!this.active(topic, entry)) return;
    this.publish(entry, { status: 'error', error: message });
    entry.controller.abort();
  }

  private lost(entry: Entry, reason: string, frame?: WireFrame): void {
    this.publish(entry, { status: 'lost', error: reason, ...(frame ? { frame } : {}) });
  }

  private accept(entry: Entry, envelope: DeliveryEnvelope): void {
    if (entry.token && entry.token !== envelope.subscriptionId) throw new Error('Observation subscription mismatch');
    entry.token = envelope.subscriptionId;
    const delivery = BigInt(envelope.delivery);
    if (entry.delivery !== undefined && delivery <= entry.delivery) return;
    const deliveryGap = entry.delivery !== undefined && delivery !== entry.delivery + 1n;
    entry.delivery = delivery;
    const frame = envelope.frame;
    const previous = entry.baseline;
    const boundary = entry.boundary ?? previous;
    const sameSource = !boundary || (boundary.source === frame.source && boundary.session === frame.session);
    if (!sameSource) {
      this.lost(entry, 'Observation source/session changed; reopen required');
      return;
    }
    // A loss boundary fences out delayed frames from the retired generation.
    if (boundary && (BigInt(frame.generation) < BigInt(boundary.generation)
        || (frame.generation === boundary.generation && BigInt(frame.revision) < BigInt(boundary.revision)))) return;
    if (frame.kind === 'loss') {
      entry.boundary = frame;
      this.lost(entry, frame.reason ?? 'Observation continuity lost', frame);
      return;
    }
    const sameIdentity = previous && previous.source === frame.source && previous.session === frame.session
      && previous.generation === frame.generation;
    if (frame.kind === 'snapshot') {
      if (boundary && (frame.generation !== boundary.generation
          || (!entry.boundary && !sameIdentity)
          || (previous?.sequence !== undefined && frame.sequence !== undefined
            && frame.generation === previous.generation && BigInt(frame.sequence) < BigInt(previous.sequence)))) {
        this.lost(entry, 'Observation baseline identity changed; reopen required');
        return;
      }
      entry.baseline = frame;
      entry.boundary = undefined;
      this.publish(entry, { status: 'ready', frame });
      return;
    }
    if (deliveryGap || !sameIdentity || entry.state.status !== 'ready'
        || BigInt(frame.revision) <= BigInt(previous!.revision)
        || (previous!.sequence !== undefined && (frame.sequence === undefined
          || BigInt(frame.sequence) !== BigInt(previous!.sequence) + 1n))) {
      this.lost(entry, 'Observation gap or source replacement; fresh baseline required');
      return;
    }
    entry.baseline = frame;
    this.publish(entry, { status: 'ready', frame });
  }
}
