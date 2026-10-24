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
  const expectedHostToken=state.host?.snapshotToken;
  if(typeof expectedHostToken!=='string'||!expectedHostToken)throw new Error('Refresh companion assignments before saving.');
  const published=await sendPublication(panel,send,{selection:{expectedHostToken,...(selected===undefined?{}:{companionIds:desired})}});
  if(published.selectionCommitted!==true)throw new Error('The native panel selection was not acknowledged. Refresh before saving.');
  if(published.panelSaveApplication?.state==='failed')throw new Error(published.panelSaveApplication.error??'The panel was saved, but its active profile could not be updated.');
  return published;
}

export async function removePanelAssignments(hashes: string[], send: CompanionSend): Promise<void> {
  const state = await send('companion_status');
  for(const instance of state.host?.instances ?? []) {
    if(typeof instance.publicationHash==='string' && hashes.includes(instance.publicationHash)) await send('companion_unassign',{instanceId:stableCompanionId(instance.instanceId)});
  }
}
