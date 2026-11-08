import assert from 'node:assert/strict';
import {until} from '../../../scripts/reactive/cdp.mjs';

export async function checkBackupSettings({page,request,clickTab,backupReader,backupRow,baselines,statusReads,waitForReader,waitForRetirement}){
  const setHidden=hidden=>page.evaluate(`(() => {
    if (${hidden}) Object.defineProperty(document, 'hidden', {configurable:true, value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  let backup;
  let reader;
  try{
    await waitForReader();
    const firstReader=backupReader();
    assert.ok(firstReader,'Backups must have one current observation while visible');
    const {storage:before}=await request({command:'storage_status'});
    assert.ok(before.backups.length<before.maximumBackups,'The private test database needs one free backup slot');
    await clickTab('Devices');
    await waitForRetirement();
    const {storage:created}=await request({command:'storage_create',expectedRevision:before.revision},45_000);
    backup=created.backup;
    assert.ok(backup?.id,'The private database must create a fixture backup');
    assert.equal(await backupRow(backup.id),false);
    await clickTab('General');
    await waitForReader();
    reader=backupReader();
    assert.notEqual(reader,firstReader,'Reopening General must create a current observation');
    await until(()=>backupRow(backup.id));

    await setHidden(true);
    await waitForRetirement().catch(cause=>{throw new Error('Hidden Backups retained its observation reader',{cause});});
    assert.equal(await backupRow(backup.id),false,'A hidden pane must withdraw its cached backup list');
    const {storage:current}=await request({command:'storage_status'});
    const {storage:removed}=await request({command:'storage_remove',id:backup.id,expectedRevision:current.revision},45_000);
    assert.equal(removed.filesRemoved,true);
    const statusReadCount=statusReads();
    await new Promise(resolve=>setTimeout(resolve,300));
    assert.equal(statusReads(),statusReadCount,'Hidden Backups must not periodically poll storage status');

    await setHidden(false);
    await waitForReader();
    const reopened=backupReader();
    assert.ok(reopened,'Visible reopen must create a current observation');
    assert.notEqual(reopened,reader,'Visible reopen must get a new observation lifetime');
    await until(()=>baselines.get(reopened)===1);
    await until(()=>backupRow(backup.id),visible=>!visible);
    assert.equal(statusReads(),statusReadCount,'A healthy reopened baseline must not query storage status');
    console.log(JSON.stringify({backupSettingsDocumentHideAndReopen:'passed',hiddenUiReads:0,reopenReads:0}));
  }finally{
    await setHidden(false);
    await clickTab('General');
    if(backup?.id){
      const {storage:current}=await request({command:'storage_status'});
      if(current.backups.some(item=>item.id===backup.id)){
        const {storage:removed}=await request({command:'storage_remove',id:backup.id,expectedRevision:current.revision},45_000);
        assert.equal(removed.filesRemoved,true);
      }
    }
  }
}

export async function checkQueuedBackupReopen({page,request,clickTab}){
  const {storage:before}=await request({command:'storage_status'});
  const {storage:{backup}}=await request({command:'storage_create',expectedRevision:before.revision},45_000);
  const click = selector => page.evaluate(`(() => {
    const button=document.querySelector(${JSON.stringify(selector)});
    if (!button || button.disabled) return false;
    button.click();
    return true;
  })()`);
  const ready = () => page.evaluate(`[...document.querySelectorAll('[aria-label="Backups"] [role="status"] strong')]
    .some(label => label.textContent.trim()==='Restore ready')`);
  await until(() => click(`[data-backup-id="${backup.id}"] button:first-child`));
  await until(() => click('dialog[open] .primary-button'));
  await until(ready);
  await clickTab('Devices');
  assert.equal(await ready(),false);
  let reads=0;
  const stop=page.listen(({method,params})=>{
    if(method!=='Network.requestWillBeSent'||!params.request.postData
      ||new URL(params.request.url).pathname!=='/gateway_request')return;
    const body=JSON.parse(params.request.postData);
    if((body.request??body).command==='storage_status')reads++;
  });
  try{
    await clickTab('General');
    await until(ready);
    assert.equal(reads,1,'A stopped runtime needs one status read to recover the committed restore');
    console.log(JSON.stringify({queuedBackupReopen:'passed',statusReads:reads}));
  }finally{stop();}
}
