import { SharedObservations } from './streams/shared.ts';

let shared: SharedObservations | undefined;

/** Presence, listener and firmware consumers share exact-topic transports. */
export function companionObservations(): SharedObservations {
  return shared ??= new SharedObservations();
}
