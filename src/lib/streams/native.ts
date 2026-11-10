import { deliverValidated, errorMessage, parseDelivery, validateDelivery, type ObservationTransport } from './protocol.ts';
import { controllerChannelDecoder } from './controller-channel.ts';

export interface NativeObservationDependencies {
  invoke: <T>(command: string, args: Record<string, unknown>) => Promise<T>;
  channel: (receive: (value: unknown) => void) => unknown;
}

export function createNativeObservationTransport(dependencies?: NativeObservationDependencies): ObservationTransport {
  const waiting = new Set<(error?: string) => void>();
  let retiring = 0;
  let retirementError: string | undefined;
  const retired = () => {
    if (--retiring) return;
    const error = retirementError;
    retirementError = undefined;
    for (const open of waiting) {
      if (retiring) break;
      waiting.delete(open);
      open(error);
    }
  };
  return { open(topic, observer, signal) {
    if (signal.aborted) return;
    void (async () => {
      let token: string | undefined;
      let opening: Promise<string> | undefined;
      let queued: ((error?: string) => void) | undefined;
      let retirementStarted = false;
      let pending: unknown;
      let pendingSet = false;
      let active = !signal.aborted;
      let inFlight = false;
      let acknowledging = false;
      let deps = dependencies;
      const expand = topic.startsWith('controllers.input.') ? controllerChannelDecoder() : undefined;
      const fail = (error: unknown) => {
        if (!active) return;
        stop();
        observer.error(errorMessage(error));
      };
      const retire = () => {
        if (!opening || retirementStarted || !deps) return;
        retirementStarted = true;
        retiring++;
        const invoke = deps.invoke;
        const unsubscribe = async (id: string) => invoke('observation_unsubscribe', { subscriptionId: id });
        const cancellation = token ? unsubscribe(token)
          : opening.then(id => typeof id === 'string' && id ? unsubscribe(id) : undefined, () => {});
        void cancellation.catch(error => { retirementError ??= errorMessage(error); }).finally(retired);
      };
      const stop = () => {
        active = false;
        pending = undefined;
        signal.removeEventListener('abort', stop);
        if (queued) waiting.delete(queued);
        retire();
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
            const canonical = expand ? expand(envelope) : envelope;
            // The decoder owns its baseline and restores detached records.
            // The expanded frame must still fit the canonical wire budgets.
            if (canonical !== envelope) validateDelivery(canonical, topic);
            inFlight = true;
            let acknowledged = false;
            deliverValidated(observer, canonical, async () => {
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
        const transport = deps;
        const subscribe = async () => {
          if (!active) return;
          opening = transport.invoke<string>('observation_subscribe', { topic, channel: transport.channel(receive) });
          if (!active) retire();
          const opened = await opening;
          if (typeof opened !== 'string' || !opened) throw new Error('Invalid observation subscription token');
          if (!active) return;
          token = opened;
          if (pendingSet) {
            const value = pending;
            pending = undefined;
            pendingSet = false;
            receive(value);
          }
        };
        queued = error => {
          if (error !== undefined) fail(error);
          else void subscribe().catch(fail);
        };
        if (retiring) waiting.add(queued);
        else queued();
      } catch (error) { fail(error); }
    })();
  } };
}
