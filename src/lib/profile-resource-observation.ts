import type { ProfileInventoryClient, ResourcePage } from './profile-inventory';
import type { DeepReadonly } from './profiles';
import type { ObservationListener } from './streams/shared';

type CatalogueHeader = { session: string; revision: string; inventoryDiagnostics: string[]; readOnly: boolean };
type Subscribe = (listener: ObservationListener) => () => void;

function catalogueHeader(payload: unknown): CatalogueHeader | undefined {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload) || Object.keys(payload).length !== 4) return;
  const header = payload as Partial<CatalogueHeader>;
  if (typeof header.session !== 'string' || !header.session || header.session.length > 4096 ||
      typeof header.revision !== 'string' || !/^(0|[1-9][0-9]{0,19})$/.test(header.revision) ||
      BigInt(header.revision) > 18446744073709551615n ||
      !Array.isArray(header.inventoryDiagnostics) || header.inventoryDiagnostics.length > 64 ||
      !header.inventoryDiagnostics.every(value => typeof value === 'string' && [...value].length <= 512) ||
      typeof header.readOnly !== 'boolean') return;
  return header as CatalogueHeader;
}

function sameCatalogue(left: { session: string; revision: string } | undefined, right: CatalogueHeader): boolean {
  return left?.session === right.session && left.revision === right.revision;
}

/** Read cached pages only on catalogue commits, with one in-flight reconstruction. */
export function followProfileResources(
  subscribe: Subscribe,
  client: Pick<ProfileInventoryClient, 'listAll'>,
  receive: (page: DeepReadonly<ResourcePage> | null, error?: string) => void,
  visible: () => boolean,
): { refreshVisible: () => void; dispose: () => void } {
  let latest: CatalogueHeader | undefined;
  let cached: DeepReadonly<ResourcePage> | undefined;
  let version = 0, attempted = -1, running = false, disposed = false;

  function publish(page: DeepReadonly<ResourcePage>, header: CatalogueHeader) {
    receive({ ...page, inventoryDiagnostics: header.inventoryDiagnostics, readOnly: header.readOnly });
  }

  async function read() {
    if (running || disposed || !visible() || !latest || attempted === version) return;
    const header = latest;
    const requestVersion = attempted = version;
    if (cached && sameCatalogue(cached, header)) {
      publish(cached, header);
      return;
    }
    running = true;
    try {
      const page = await client.listAll();
      if (disposed || !latest || requestVersion !== version) return;
      if (!sameCatalogue(page, header)) throw new Error('Waiting for the latest resource inventory revision.');
      cached = page;
      publish(page, latest);
    } catch (error) {
      if (!disposed && requestVersion === version) {
        receive(null, error instanceof Error ? error.message : String(error));
      }
    } finally {
      running = false;
      void read();
    }
  }

  const unsubscribe = subscribe(state => {
    if (disposed || state.status === 'connecting') return;
    const header = state.status === 'ready' && state.frame?.source === 'profile-resources'
      ? catalogueHeader(state.frame.payload)
      : undefined;
    if (!header) {
      latest = undefined;
      version++;
      receive(null, state.error ?? 'Waiting for the current resource inventory.');
      return;
    }
    if (!sameCatalogue(latest, header)) version++;
    latest = header;
    if (cached && attempted === version && sameCatalogue(cached, header)) publish(cached, header);
    else void read();
  });

  return {
    refreshVisible: () => { void read(); },
    dispose: () => { disposed = true; unsubscribe(); },
  };
}
