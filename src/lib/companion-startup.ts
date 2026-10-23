import type { Publication } from '@opensimgear/companion-designer';
import type { CompanionHostState } from './companions';
import {latestSavedPanels,type CompanionLibrarySnapshot} from './companion-library.ts';
import type { CompiledWorkspace } from './compiled-companion-library.ts';
import type { CompanionPublication } from './companions.ts';

export async function restoreAssignedPanels<T extends CompanionPublication>(state: CompanionHostState, load: () => Promise<T[]>, publish: (panel: T) => Promise<unknown>) {
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
export async function restoreCompiledPanelLibrary(state: CompanionHostState, snapshot: CompiledWorkspace, publish: (panel: import('../../packages/widget-runtime/src/panel.ts').CompiledPanelPublication, options: { saveAsLatest?: boolean }) => Promise<unknown>) {
  await restoreAssignedPanels(state, async () => Object.values(snapshot.publications).flat(), panel => publish(panel, { saveAsLatest: false }));
  for (const draft of snapshot.drafts) { const panel = snapshot.publications[draft.id]?.at(-1); if (panel) await publish(panel, { saveAsLatest: true }); }
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
    const { CompiledWorkspaceStore } = await import('./compiled-companion-library');
    const snapshot = await new CompiledWorkspaceStore().load();
    await restoreCompiledPanelLibrary(state,snapshot,(panel,options)=>sendPublication(panel,send,options));
  })().catch(error => { restoring = undefined; throw error; });
}
