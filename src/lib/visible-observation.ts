import type { SharedObservations, ObservationListener } from './streams/shared';

/** Retain one shared topic subscription only while its owning view and document are visible. */
export function visibleObservation(observations: Pick<SharedObservations, 'subscribe'>, topic: string, listener: ObservationListener) {
  let requested = false;
  let closed = false;
  let stop: (() => void) | undefined;
  const sync = () => {
    const shouldSubscribe = requested && !closed && !document.hidden;
    if (shouldSubscribe && !stop) stop = observations.subscribe(topic, listener);
    else if (!shouldSubscribe && stop) { stop(); stop = undefined; }
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
