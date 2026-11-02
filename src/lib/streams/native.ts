import { errorMessage, parseDelivery, type ObservationTransport } from './protocol.ts';

export interface NativeObservationDependencies {
  invoke: <T>(command: string, args: Record<string, unknown>) => Promise<T>;
  channel: (receive: (value: unknown) => void) => unknown;
}

export function createNativeObservationTransport(dependencies?: NativeObservationDependencies): ObservationTransport {
  return { open(topic, observer, signal) {
    if (signal.aborted) return;
    void (async () => {
      let token: string | undefined;
      let pending: unknown;
      let pendingSet = false;
      let active = !signal.aborted;
      let inFlight = false;
      let acknowledging = false;
      let deps = dependencies;
      const fail = (error: unknown) => {
        if (!active) return;
        observer.error(errorMessage(error));
        stop();
      };
      const stop = () => {
        active = false;
        pending = undefined;
        signal.removeEventListener('abort', stop);
        if (token && deps) {
          const closing = token;
          token = undefined;
          void deps.invoke('observation_unsubscribe', { subscriptionId: closing }).catch(() => {});
        }
      };
      signal.addEventListener('abort', stop, { once: true });
      try {
        if (!deps) {
          const core = await import('@tauri-apps/api/core');
          deps = { invoke: core.invoke, channel(receive) {
            const channel = new core.Channel<unknown>();
            channel.onmessage = receive;
            return channel;
          } };
        }
        if (!active) return;
        const receive = (value: unknown) => {
          if (!active) return;
          if (!token) {
            if (pendingSet) return fail(new Error('Observation exceeded one in-flight delivery'));
            pending = value;
            pendingSet = true;
            return;
          }
          try {
            if (inFlight) {
              if (!acknowledging || pendingSet) throw new Error('Observation exceeded one in-flight delivery');
              pending = value;
              pendingSet = true;
              return;
            }
            const envelope = parseDelivery(value, topic);
            if (envelope.subscriptionId !== token) throw new Error('Observation subscription mismatch');
            inFlight = true;
            let acknowledged = false;
            observer.frame(envelope, async () => {
              if (acknowledged || !active) return;
              acknowledged = true;
              acknowledging = true;
              try {
                await deps!.invoke('observation_acknowledge', {
                  subscriptionId: envelope.subscriptionId, delivery: envelope.delivery,
                });
                inFlight = false;
                acknowledging = false;
                if (pendingSet) {
                  const next = pending;
                  pending = undefined;
                  pendingSet = false;
                  receive(next);
                }
              } catch (error) { fail(error); }
            });
          } catch (error) { fail(error); }
        };
        const opened = await deps.invoke<string>('observation_subscribe', { topic, channel: deps.channel(receive) });
        if (typeof opened !== 'string' || !opened) throw new Error('Invalid observation subscription token');
        token = opened;
        if (!active) { stop(); return; }
        if (pendingSet) {
          const value = pending;
          pending = undefined;
          pendingSet = false;
          receive(value);
        }
      } catch (error) { fail(error); }
    })();
  } };
}
