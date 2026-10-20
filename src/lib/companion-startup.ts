import type { Publication } from '@opensimgear/companion-designer';
import type { CompanionHostState } from './companions';
import {latestSavedPanels,type CompanionLibrarySnapshot} from './companion-library.ts';

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
/** Transport-only restoration never changes a panel's saved state. */
export async function restoreSavedPanelLibrary(state:CompanionHostState,snapshot:CompanionLibrarySnapshot,publish:(panel:Publication,options:{saveAsLatest?:boolean})=>Promise<unknown>){
  await restoreAssignedPanels(state,async()=>Object.values(snapshot.publications).flat(),panel=>publish(panel,{saveAsLatest:false}));
  for(const panel of latestSavedPanels(snapshot.library,snapshot.publications))await publish(panel,{saveAsLatest:true});
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
    const { CompanionLibraryStore, indexedDbStorage } = await import('./companion-library');
    const snapshot = await new CompanionLibraryStore(indexedDbStorage()).load();
    await restoreSavedPanelLibrary(state,snapshot,(panel,options)=>sendPublication(panel,send,options));
  })().catch(error => { restoring = undefined; throw error; });
}
