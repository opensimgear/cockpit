import { companionPanelReason } from './companion-screen.ts';
import { sendPublication, stableCompanionId, type CompanionSend, type CompanionHostState, type CompanionPublication } from './companions.ts';

/** Mutable panel identities use immutable transport snapshots internally. */
export async function synchronizePanel(panel: CompanionPublication, previousHashes: string[], send: CompanionSend, selected?: string[]): Promise<CompanionHostState> {

  let state = await send('companion_status');
  const hashes = new Set([panel.publicationHash,...previousHashes]);
  for(const p of state.host?.publications ?? []) if(p.uiId===panel.document.ui.id && typeof p.publicationHash==='string') hashes.add(p.publicationHash);
  const instances = state.host?.instances ?? [];
  const profileOwnedCompanions=new Set(instances.filter(instance=>instance.profileOwned===true).map(instance=>instance.companionId));
  const owned = instances.filter(i => typeof i.publicationHash==='string' && hashes.has(i.publicationHash));
  const desired = selected === undefined ? [...new Set(owned.map(i => stableCompanionId(i.companionId)))] : [...new Set(selected.map(stableCompanionId))];
  for(const companionId of desired) {
    if(selected!==undefined && !(state.host?.clients ?? []).some(c => c.companionId===companionId)) throw new Error('The selected companion is no longer paired.');
    if(!owned.some(i => i.companionId===companionId) && instances.filter(i => i.companionId===companionId).length>1) throw new Error('This companion has multiple controller assignments. Disconnect its extra assignments before assigning a panel.');
  }
  for(const id of desired)if(!profileOwnedCompanions.has(id)){
    const reason=companionPanelReason(panel,state,id);if(reason)throw new Error(reason);
  }
  const published=await sendPublication(panel,send);
  if(published.panelSaveApplication?.state==='failed')throw new Error(published.panelSaveApplication.error??'The panel was saved, but its active profile could not be updated.');
  // Native commit may already have replaced profile and direct assignments.
  // Re-read ownership before any legacy/direct selection changes.
  state=await send('companion_status');
  const currentInstances=state.host?.instances??[];
  const currentOwned=currentInstances.filter(instance=>typeof instance.publicationHash==='string'&&hashes.has(instance.publicationHash));
  const currentProfileOwned=new Set(currentInstances.filter(instance=>instance.profileOwned===true).map(instance=>instance.companionId));
  for(const instance of currentOwned) {
    if(instance.profileOwned===true||currentProfileOwned.has(instance.companionId))continue;
    if(!desired.includes(stableCompanionId(instance.companionId))) await send('companion_unassign',{instanceId:stableCompanionId(instance.instanceId)});
  }
  for(const companionId of desired) {
    if(currentProfileOwned.has(companionId))continue;
    const own = currentOwned.filter(i => i.companionId===companionId);
    if(selected===undefined&&!own.length)continue;
    const existing = currentInstances.filter(i => i.companionId===companionId);
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
