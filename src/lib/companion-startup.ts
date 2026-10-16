import type { Publication } from '@opensimgear/companion-designer';
import type { CompanionHostState } from './companions';

export async function restoreAssignedPanels(state: CompanionHostState, load: () => Promise<Publication[]>, publish: (panel: Publication) => Promise<unknown>) {
  const available = new Set((state.host?.publications ?? []).map(p => p.publicationHash));
  const missing = new Set((state.host?.instances ?? []).map(a => a.publicationHash).filter(hash => !available.has(hash)));
  if (!missing.size) return;
  const saved = await load();
  const panels = [...missing].map(hash => {
    const panel = saved.find(p => p.publicationHash === hash);
    if (!panel) throw new Error('An assigned panel could not be restored from local storage. Open Companions and select a saved panel.');
    return panel;
  });
  for (const panel of panels) await publish(panel);
}
let restoring: Promise<void> | undefined;
/** Shared by app startup and the designer; a single upload owns native IPC. */
export function restoreCompanionPanels(): Promise<void> {
  return restoring ??= (async () => {
    const { gatewayRequest } = await import('./gateway');
    const { sendPublication } = await import('./companions');
    const send = async (command: import('./companions').CompanionCommand, companion: Record<string, unknown> = {}) => {
      const result = (await gatewayRequest({ command, companion })).companions;
      if (!result) throw new Error('Companion panels require the native Cockpit app.');
      return result;
    };
    const state = await send('companion_status');
    await restoreAssignedPanels(state, async () => {
      const { CompanionLibraryStore, indexedDbStorage } = await import('./companion-library');
      const snapshot = await new CompanionLibraryStore(indexedDbStorage()).load();
      return Object.values(snapshot.publications).flat();
    }, panel => sendPublication(panel, send));
  })().catch(error => { restoring = undefined; throw error; });
}
