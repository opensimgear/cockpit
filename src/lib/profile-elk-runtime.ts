/// <reference types="vite/client" />
import ELK from 'elkjs/lib/elk-api.js';
import type { ELK as ElkEngine } from 'elkjs/lib/elk-api';
import ElkWorker from 'elkjs/lib/elk-worker.min.js?worker';
import type { DiagramNode } from './profile-diagram';
import type { DeepReadonly, ProfileBinding } from './profiles';
import { layoutElkDiagram, type ProfileElkLayout } from './profile-elk-layout';

let engine: ElkEngine | undefined;
let worker: Worker | undefined;
const pending = new Set<(error: Error) => void>();
function stopEngine(error: Error) {
  const current = worker;
  engine = undefined; worker = undefined;
  current?.terminate();
  for (const reject of [...pending]) reject(error);
  pending.clear();
}
function getEngine(): ElkEngine {
  if (!engine) engine = new ELK({
    algorithms: ['layered'],
    workerFactory: () => {
      const current = new ElkWorker();
      worker = current;
      current.addEventListener('error', () => {
        if (worker === current) stopEngine(new Error('The ELK layout worker stopped. Please try Auto layout again.'));
      });
      current.addEventListener('messageerror', () => {
        if (worker === current) stopEngine(new Error('The ELK layout worker returned an unreadable response. Please try again.'));
      });
      return current;
    }
  });
  return engine;
}

/** The expensive layered search stays in a same-origin worker. A failed or
 * timed-out worker is discarded so the next request gets a fresh engine. */
export async function layoutProfileWithElk(nodes: readonly DiagramNode[], bindings: readonly DeepReadonly<Pick<ProfileBinding, 'id' | 'from' | 'to'>>[], width: number, height: number): Promise<ProfileElkLayout> {
  const current = getEngine();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let rejectRequest: ((error: Error) => void) | undefined;
  const interrupted = new Promise<never>((_, reject) => {
    rejectRequest = reject;
    pending.add(reject);
    timer = setTimeout(() => stopEngine(new Error('ELK layout took too long. Please try Auto layout again.')), 30_000);
  });
  try {
    return await Promise.race([layoutElkDiagram(nodes, bindings, width, height, current), interrupted]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    if (rejectRequest) pending.delete(rejectRequest);
  }
}
