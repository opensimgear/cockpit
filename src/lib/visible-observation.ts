import type { SharedObservations, ObservationListener } from './streams/shared';

/** Retain a subscription only while its owning view and document are visible. */
export function visibleSubscription(subscribe: () => () => void, retired?: () => void) {
  let requested = false;
  let closed = false;
  let stop: (() => void) | undefined;
  const sync = () => {
    const shouldSubscribe = requested && !closed && !document.hidden;
    if (shouldSubscribe && !stop) stop = subscribe();
    else if (!shouldSubscribe && stop) {
      stop();
      stop = undefined;
      retired?.();
    }
  };
  document.addEventListener('visibilitychange', sync);
  return {
    setVisible(visible: boolean) { requested = visible; sync(); },
    close() {
      closed = true;
      sync();
      document.removeEventListener('visibilitychange', sync);
    },
  };
}

export function visibleObservation(
  observations: Pick<SharedObservations, 'subscribe'>,
  topic: string,
  listener: ObservationListener,
  retired?: () => void,
) {
  return visibleSubscription(() => observations.subscribe(topic, listener), retired);
}
