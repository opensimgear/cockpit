import { ProfileInventoryClient, type ResourcePage } from './profile-inventory';
import { followProfileResources } from './profile-resource-observation';
import type { DeepReadonly } from './profiles';
import { SharedObservations } from './streams/shared';

type Listener = (page: DeepReadonly<ResourcePage> | null, error?: string) => void;
type Catalogue = {
  listeners: Set<Listener>;
  last?: Parameters<Listener>;
  stop?: () => void;
};

let current: Catalogue | undefined;

/** Share the exact committed catalogue and its paged reconstruction across views. */
export function observeProfileResources(listener: Listener): () => void {
  let entry = current;
  if (!entry) {
    const created: Catalogue = { listeners: new Set() };
    current = entry = created;
    const observations = new SharedObservations();
    const client = new ProfileInventoryClient(async request => {
      const { gatewayRequest } = await import('./gateway');
      const result = (await gatewayRequest(request)).profileResources;
      if (!result) throw new Error('Resource inventory is unavailable. Reconnect the native service.');
      return result;
    });
    const subscription = followProfileResources(
      receive => observations.subscribe('profiles.resources', receive), client,
      (...value) => {
        created.last = value;
        for (const receive of [...created.listeners]) {
          if (!created.listeners.has(receive)) continue;
          try { receive(...value); } catch { /* A view cannot prevent other views from updating. */ }
        }
      },
      () => !document.hidden,
    );
    document.addEventListener('visibilitychange', subscription.refreshVisible);
    created.stop = () => {
      document.removeEventListener('visibilitychange', subscription.refreshVisible);
      subscription.dispose();
    };
  }
  const catalogue = entry;
  const receive: Listener = (...value) => listener(...value);
  catalogue.listeners.add(receive);
  if (catalogue.last) {
    try { receive(...catalogue.last); } catch { /* Isolate a newly attached view. */ }
  }
  return () => {
    catalogue.listeners.delete(receive);
    if (current === catalogue && !catalogue.listeners.size) {
      current = undefined;
      catalogue.stop?.();
    }
  };
}
