import { isTauri } from '@tauri-apps/api/core';
import { createBrowserObservationTransport } from './browser.ts';
import { createNativeObservationTransport } from './native.ts';
import type { ObservationTransport } from './protocol.ts';

/** Choose once for this shared client's lifetime. */
export function createObservationTransport(): ObservationTransport {
  return isTauri() ? createNativeObservationTransport() : createBrowserObservationTransport();
}
