import type { Publication } from '../../packages/companion-designer/src/index.ts';
import { sendPublication, stableCompanionId, type CompanionSend, type CompanionHostState } from './companions.ts';

/** Mutable panel identities use immutable transport snapshots internally. */
export async function synchronizePanel(panel: Publication, previousHashes: string[], send: CompanionSend, selected?: string[]): Promise<CompanionHostState> {
  await sendPublication(panel,send);
  let state = await send('companion_status');
  const hashes = new Set([panel.publicationHash,...previousHashes]);
  for(const p of state.host?.publications ?? []) if(p.uiId===panel.document.ui.id && typeof p.publicationHash==='string') hashes.add(p.publicationHash);
  const instances = state.host?.instances ?? [];
  const owned = instances.filter(i => typeof i.publicationHash==='string' && hashes.has(i.publicationHash));
  const desired = selected === undefined ? [...new Set(owned.map(i => stableCompanionId(i.companionId)))] : [...new Set(selected.map(stableCompanionId))];
  for(const companionId of desired) {
    if(selected!==undefined && !(state.host?.clients ?? []).some(c => c.companionId===companionId)) throw new Error('The selected companion is no longer paired.');
    if(!owned.some(i => i.companionId===companionId) && instances.filter(i => i.companionId===companionId).length>1) throw new Error('This companion has multiple controller assignments. Disconnect its extra assignments before assigning a panel.');
  }
  for(const instance of owned) {
    if(!desired.includes(stableCompanionId(instance.companionId))) await send('companion_unassign',{instanceId:stableCompanionId(instance.instanceId)});
  }
  for(const companionId of desired) {
    const own = owned.filter(i => i.companionId===companionId);
    const existing = instances.filter(i => i.companionId===companionId);
    const targets = own.length ? own : existing.length ? existing : [{instanceId:crypto.randomUUID(),publicationHash:undefined}];
    for(const instance of targets) {
      if(instance.publicationHash===panel.publicationHash) continue;
      await send('companion_assign',{companionId,instanceId:stableCompanionId(instance.instanceId),publicationHash:panel.publicationHash});
    }
  }
  state = await send('companion_status');
  return state;
}

export async function removePanelAssignments(hashes: string[], send: CompanionSend): Promise<void> {
  const state = await send('companion_status');
  for(const instance of state.host?.instances ?? []) {
    if(typeof instance.publicationHash==='string' && hashes.includes(instance.publicationHash)) await send('companion_unassign',{instanceId:stableCompanionId(instance.instanceId)});
  }
}
