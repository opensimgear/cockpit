// Real running development Cockpit and native IPC. No intercepted transport,
// fixture acknowledgements, enablement changes or output activation.
// Key capture uses the real native Pause; cleanup restores its prior value.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire}from'node:module';
import {mkdir,writeFile}from'node:fs/promises';
import {resolve}from'node:path';
import {desktopPreviewFrames,previewSourceKey,desktopPreviewResult}from'../../src/lib/profile-desktop-preview.ts';
const require=createRequire(import.meta.url),playwrightModule=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(playwrightModule),{expect}=require(playwrightModule+'/test');
const endpoint=process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222';
const artifacts=resolve('.research/desktop-output-native-authoring');
test('running native Cockpit authors and roundtrips a desktop endpoint without output activation',{timeout:180_000},async()=>{
  await mkdir(artifacts,{recursive:true});
  const browser=await chromium.connectOverCDP(endpoint);
  const pages=browser.contexts().flatMap(context=>context.pages());
  const page=pages.find(page=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()))??pages[0];
  assert.ok(page,'A running native Cockpit page must already exist.');
  const native=async request=>{
    const result=await page.evaluate(request=>{
      if(!window.__TAURI_INTERNALS__?.invoke)throw Error('This is not the native Cockpit.');
      return window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'desktop-native-authoring-check'}});
    },request);
    assert.equal(result.ok,true,result.error);assert.ok(result.state);return result.state;
  };
  const before=await native({command:'profile_status'}),desktopBefore=await native({command:'profile_desktop_status'});
  const name=`Native desktop authoring ${Date.now()}`;
  const createdNames=new Set([name,`${name} imported`,`${name} recording`]);
  try{
    const openExport=page.locator('.library-dialog').getByRole('button',{name:'Close',exact:true});if(await openExport.count())await openExport.click();
    const nav=page.getByRole('button',{name:'Profiles',exact:true});
    if(await nav.count())await nav.click();
    const back=page.getByRole('button',{name:/Back to Profiles/});
    if(await back.count()){
      await back.click();const discard=page.getByRole('alertdialog').getByRole('button',{name:'Discard and continue',exact:true});if(await discard.count())await discard.click();
    }
    await page.getByRole('button',{name:'Add profile',exact:true}).click();
    await page.getByLabel('Profile name',{exact:true}).fill(name);
    await page.getByLabel('Keyboard and mouse preset',{exact:true}).selectOption('key-hold');
    await page.getByRole('button',{name:'Add keyboard and mouse',exact:true}).click();
    const inspector=page.getByRole('region',{name:'Keyboard and mouse action editor'});
    await expect(inspector).toBeVisible();
    await inspector.getByRole('button',{name:'Capture a physical key',exact:true}).click();
    const picker=inspector.getByLabel('Physical key capture',{exact:true});
    await expect(picker).toBeFocused();
    const capturePause=(await native({command:'profile_status'})).profiles.profileSession;
    assert.equal(capturePause.paused,true,'Native Pause must finish before the picker receives focus');
    // No click or explicit focus call: the very first key must reach the picker.
    await page.keyboard.press('ArrowRight');
    await expect(inspector.getByLabel('Physical key',{exact:true})).toHaveValue('79');
    await expect(picker).toHaveCount(0);
    assert.equal((await native({command:'profile_status'})).profiles.profileSession.paused,true,'Capturing a key must leave output paused');
    await native({command:'profile_pause',paused:before.profiles.profileSession.paused});
    await inspector.getByRole('button',{name:'Update desktop actions',exact:true}).click();
    await page.getByRole('button',{name:'Save profile',exact:true}).click();
    await expect(page.getByText(/Saved revision 1 · Editing this profile does not activate it/)).toBeVisible();
    await page.getByRole('button',{name:/Back to Profiles/}).click();
    await page.getByRole('button',{name:`Edit ${name}`,exact:true}).click();
    await page.getByRole('button',{name:'Export saved profile',exact:true}).click();
    const exported=page.getByLabel('Exported profile JSON',{exact:true});await expect(exported).toBeVisible();
    const document=JSON.parse(await exported.inputValue());
    assert.equal(document.desktopOutputs.length,1);assert.equal(document.virtualOutputs.length,0);
    assert.equal(document.desktopOutputs[0].actions[0].action.key,79);
    assert.equal(document.desktopOutputs[0].ports[0].contract.delivery,'state');
    assert.equal(document.enabled,undefined);assert.equal(document.scope,undefined);
    await page.getByRole('alertdialog').getByRole('button',{name:'Close',exact:true}).click();
    const compiled=await native({command:'profile_compile',document,assignments:[]});
    assert.notEqual(compiled.profileCompilation.readiness,'invalid');
    assert.equal(compiled.profileCompilation.budgets.virtualTargets,0);
    // Unconnected editing is valid, and requests no executable backend yet.
    assert.equal(compiled.profileCompilation.requirements?.virtualDevices,false);
    // Record an unsaved synthetic source against the real native compiler. Its
    // absent machine assignment grants no acquisition or submission authority.
    const recordingDocument=structuredClone(document),hold=recordingDocument.desktopOutputs[0].ports[0];
    recordingDocument.resources=[{id:'preview-source',name:'Synthetic recording source',kind:'device',required:false,ports:[{...structuredClone(hold),id:'button',name:'Recorded button',direction:'output'}]}];
    recordingDocument.processingNodes=[];recordingDocument.bindings=[{id:'recorded-key',from:{nodeId:'preview-source',portId:'button'},to:{nodeId:recordingDocument.desktopOutputs[0].id,portId:hold.id}}];recordingDocument.editor.positions=[];
    const recordingCompilation=(await native({command:'profile_compile',document:recordingDocument,assignments:[]})).profileCompilation;
    const sources=(await native({command:'profile_compilation_inspect',compilationId:recordingCompilation.compilationId,section:'sources',offset:0,limit:64})).profileCompilation.items;
    assert.equal(sources.length,1);const sample=desktopPreviewFrames(sources,previewSourceKey(sources[0].endpoint),true);
    const recording=desktopPreviewResult((await native({command:'profile_desktop_preview',compilationId:recordingCompilation.compilationId,...sample})).profileDesktopPreview,recordingCompilation);
    assert.equal(recording.nativeSubmission,false);assert.ok(recording.operations.some(operation=>operation.events.some(event=>event.kind==='key'&&event.down===true)));assert.ok(recording.operations.some(operation=>operation.events.some(event=>event.kind==='key'&&event.down===false)));
    const imported=await native({command:'profile_import',document,name:`${name} imported`});
    assert.notEqual(imported.profiles.document.id,document.id);
    assert.deepEqual(imported.profiles.document.desktopOutputs,document.desktopOutputs);
    assert.deepEqual(imported.profiles.document.virtualOutputs,[]);
    const recordingName=`${name} recording`,recordingSaved=(await native({command:'profile_import',document:recordingDocument,name:recordingName})).profiles.document;
    await page.getByRole('button',{name:/Back to Profiles/}).click();
    await page.getByRole('button',{name:'Refresh profiles',exact:true}).click();
    await page.getByRole('button',{name:`Edit ${recordingName}`,exact:true}).click();
    await page.getByRole('button',{name:'Inspect draft preflight',exact:true}).click();
    const recordingUi=page.getByRole('region',{name:'Keyboard and mouse recording preview',exact:true});
    await expect(recordingUi).toBeVisible();
    await recordingUi.getByRole('button',{name:'Load captured source controls',exact:true}).click();
    await expect(recordingUi.getByLabel('Exact recorded source',{exact:true})).toHaveValue(previewSourceKey({nodeId:'preview-source',portId:'button'}));
    await expect(recordingUi.getByLabel('Synthetic pressed value',{exact:true})).toBeChecked();
    await recordingUi.getByRole('button',{name:'Record without submitting input',exact:true}).click();
    await expect(recordingUi.getByText('No native submission',{exact:true})).toBeVisible();
    await expect(recordingUi.getByRole('table')).toContainText('recorded-key');
    await recordingUi.screenshot({path:resolve(artifacts,'recording-preview-ui.png')});
    const after=await native({command:'profile_status'}),desktopAfter=await native({command:'profile_desktop_status'});
    assert.deepEqual(after.profiles.profileSession,before.profiles.profileSession);
    assert.equal(desktopAfter.desktop.enabled,desktopBefore.desktop.enabled);
    assert.deepEqual(desktopAfter.desktop.scope,desktopBefore.desktop.scope);
    await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify({name,document,importedId:imported.profiles.document.id,recordingUiProfileId:recordingSaved.id,recordingUiVerified:true,keyCapture:{focusedWithoutManualFocus:true,firstKey:'ArrowRight',capturedUsage:79,nativePausedBeforeFocus:capturePause.paused,remainedPausedAfterKey:true},compilation:compiled.profileCompilation,recording,before:before.profiles.profileSession,after:after.profiles.profileSession,desktop:desktopAfter.desktop},null,2)+'\n');
    await page.screenshot({path:resolve(artifacts,'desktop-profile-reopened.png'),fullPage:true});
  }catch(error){await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true});throw error;}
  finally {
    try {
      await native({command:'profile_pause',paused:before.profiles.profileSession.paused});
      const library=(await native({command:'profile_list'})).profiles;
      const created=library.profiles.filter(profile=>createdNames.has(profile.name));
      for(const profile of created)await native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
      await writeFile(resolve(artifacts,'cleanup.json'),JSON.stringify({deletedTestProfiles:created.length,pausedRestored:before.profiles.profileSession.paused},null,2)+'\n');
    } finally { await browser.close(); } // Disconnect CDP; the native app stays open.
  }
});
