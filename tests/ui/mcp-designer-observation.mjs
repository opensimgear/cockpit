import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';

export const designerUri = 'cockpit://observations/designer.workspace';

export async function designerObservation({page, reader, nextObservation, evidence}) {
  const request = input => page.invoke('persistence_workspace', {request:input});
  const read = () => request({operation:'read'});
  const write = (state, drafts) => request({
    operation:'write', expectedRevision:state.revision, expectedLibraryRevision:state.libraryRevision,
    value:{...state.value, drafts},
  });
  const id = 'panel-' + randomUUID();
  const draft = {id,title:'Temporary designer observation',pages:[{id:'main',title:'Main',width:1280,height:800}],widgets:[]};
  try {
    const initial = await read();
    const committed = await write(initial, [...initial.value.drafts, draft]);
    const payload = {commit:{revision:committed.revision,libraryRevision:committed.libraryRevision}};
    let update;
    do {
      update = await nextObservation();
    } while (update.params?.uri !== designerUri);
    const frame = update.params._meta['opensimgear.com/observation'];
    assert.notEqual(frame.kind, 'loss');
    assert.deepEqual(frame.payload, payload);
    const snapshot = JSON.parse((await reader.readResource({uri:designerUri})).contents[0].text);
    assert.deepEqual(snapshot.frame.payload, payload);
    evidence.designerObservation = {revisionOnly:true,streamAndSnapshotAgree:true};
  } finally {
    const current = await read();
    const drafts = current.value.drafts.filter(draft => draft.id !== id);
    if (drafts.length !== current.value.drafts.length) await write(current, drafts);
  }
}
