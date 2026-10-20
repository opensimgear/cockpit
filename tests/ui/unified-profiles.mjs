// Frontend regression with authored gateway replies. No native service, device,
// driver or game is used. Start a frontend-only Vite server before running.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';

const require = createRequire(import.meta.url);
const playwrightModule = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(playwrightModule);
const { expect } = require(playwrightModule + '/test');
const endpoint = process.env.OSG_PROFILES_UI_URL ?? 'http://127.0.0.1:5187';
const artifacts = resolve(process.env.OSG_PROFILES_UI_ARTIFACTS ?? '.research/profiles-browser-acceptance');
const defaultId = '00000000-0000-4000-8000-000000000001';
const hash = `sha256:${'1'.repeat(64)}`;
const plain = value => JSON.parse(JSON.stringify(value));
const blank = () => ({schemaVersion:2,id:defaultId,name:'Default',revision:1,eligibility:{gameIds:[]},resources:[],bindings:[],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}});

test('keyboard Add, Save, Cancel and Back preserve saved profiles and the exact applied run', {timeout:60000}, async () => {
  await mkdir(artifacts,{recursive:true});
  const driver = await readFile(new URL(import.meta.url)), driverSha256 = createHash('sha256').update(driver).digest('hex');
  await writeFile(resolve(artifacts,'driver-source.mjs'),driver);
  const browser = await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL ? {channel:process.env.OSG_BROWSER_CHANNEL} : {})});
  const page = await browser.newPage({viewport:{width:1440,height:1100}});
  const documents = new Map([[defaultId, blank()]]), requests = [], unexpected = [], errors = [], keyboard = [];
  async function activateByTab(target, name, key) {
    await expect(target).toBeVisible();await expect(target).toBeEnabled();
    const visibleStops = await page.evaluate(()=>{
      const root=document.querySelector('dialog[open]')??document;
      return [...root.querySelectorAll('a[href],button,input,select,textarea,summary,[tabindex],[contenteditable="true"]')]
        .filter(node=>node.tabIndex>=0&&!node.matches(':disabled')&&!node.closest('[inert]')&&node.getClientRects().length&&getComputedStyle(node).visibility==='visible').length;
    });
    assert.ok(visibleStops>0,'The visible DOM must expose native keyboard stops');
    // Always use a real Tab, even when showModal initially focuses the target.
    // Counting visible stops bounds one full cycle; no focus API moves focus.
    for(let tabs=1;tabs<=visibleStops+2;tabs++){
      await page.keyboard.press('Tab');
      if(await target.evaluate(node=>node===document.activeElement)){
        await expect(target).toBeFocused();
        keyboard.push({name,key,tabs,visibleStops});
        await page.keyboard.press(key);return;
      }
    }
    throw new Error(`Keyboard Tab did not reach ${name} within ${visibleStops+2} visible-DOM stops`);
  }
  const applied = {profileId:defaultId,revision:1,semanticHash:hash};
  const owner = {session:'11111111-1111-4111-8111-111111111111',generation:7,profileId:defaultId,profileRevision:1,gameId:null};
  const profileSession = {state:'running',paused:false,appliedSnapshot:applied,owner,degradation:[],fault:null};
  const session = {selectedProfileId:defaultId,selectionMode:'manual',state:'running',appliedSnapshot:applied,fault:null};
  const automatic = {schemaVersion:1,mode:'manual',manualProfileId:defaultId,rules:[],status:'manual',message:'Manual selection is pinned.',conflicts:[],lastAttempt:null,target:null,readOnly:false};
  const listing = () => ({profiles:[...documents.values()].map(document=>({id:document.id,name:document.name,revision:document.revision,isDefault:document.id===defaultId,semanticHash:hash})),session,profileSession,automatic,readOnly:false,diagnostics:[]});
  const base = () => ({running:false,needsReconnect:false,modules:[],ports:[],games:{catalogue:[],profiles:[],defaultProfiles:{},session:{phase:'idle'}},integration:{running:false},dcsBios:{running:false},companions:{host:{instances:[],publications:[],clients:[]}}});
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/api/gateway/**',async route=>{unexpected.push('Unexpected HTTP gateway fallback');await route.abort();});
  await page.exposeFunction('__osgNativeInvoke', async (command, args) => {
    if(command!=='gateway_request')throw new Error(`Unexpected native invocation ${command}`);
    const request = args.request;
    requests.push(plain(request));
    if(['profile_list','profile_status','profile_automatic_status'].includes(request.command))return {ok:true,state:{profiles:plain(listing())}};
    if(request.command==='profile_desktop_status')return {ok:true,state:{desktop:{enabled:false,qualified:false,state:'disabled',reason:'Fixture observation only.',scope:{kind:'application'}}}};
    if(request.command==='profile_resources_list')return {ok:true,state:{profileResources:{session:'fixture-inventory',revision:'1',offset:0,total:0,resources:[],inventoryDiagnostics:[],readOnly:false}}};
    if(request.command==='profile_assignments_get')return {ok:true,state:{profiles:{...plain(listing()),document:plain(documents.get(request.id)),assignments:[]}}};
    if(request.command==='profile_editor_save'){
      assert.equal(request.expectedRevision,documents.get(request.document.id)?.revision??null);
      const document={...plain(request.document),revision:request.expectedRevision===null?1:request.expectedRevision+1};
      documents.set(document.id,document);
      return {ok:true,state:{profiles:{...plain(listing()),document:plain(document),assignments:plain(request.assignments),saveApplication:{state:'inactive',error:null}}}};
    }
    if(request.command==='profile_runtime_topology')return {ok:true,state:{profileTopology:{schemaVersion:1,owner,appliedSnapshot:applied,programHash:hash,capturedUs:'0',section:request.section,offset:0,total:0,items:[]}}};
    if(request.command==='profile_runtime_inspect')return {ok:true,state:{profileInspection:{schemaVersion:1,owner,appliedSnapshot:applied,programHash:hash,capturedUs:'0',offset:0,total:0,ports:[]}}};
    if(request.command==='profile_retired_recovery_status')return {ok:true,state:{retiredRecovery:{required:false,outputs:[],provisioning:[],job:null,diagnostics:[],outputQualified:false,provisioningQualified:false}}};
    if(request.command.startsWith('profile_')){
      unexpected.push(request.command);
      return {ok:false,error:`No native effects in browser fixture: ${request.command}`};
    }
    return {ok:true,state:base()};
  });
  await page.addInitScript(()=>{window.isTauri=true;window.__TAURI_INTERNALS__={invoke:(command,args)=>window.__osgNativeInvoke(command,args)};});
  try {
    await page.goto(endpoint);
    const nav = page.getByRole('navigation',{name:'Main navigation'});
    await nav.getByRole('button',{name:'Profiles',exact:true}).click();
    await expect(page.getByRole('heading',{name:'Profiles',exact:true})).toBeVisible();
    await expect(page.getByRole('button',{name:'Edit Default',exact:true})).toBeVisible();
    await expect(page.getByRole('button',{name:'Edit Default',exact:true})).toBeInViewport();
    await expect(page.getByRole('button',{name:'Add profile',exact:true})).toBeInViewport();
    await expect(page.getByRole('button',{name:'Stop current profile',exact:true})).toBeInViewport();
    const runtime = page.getByLabel('Runtime profile status');
    await expect(runtime).toContainText('Active profile · Default');
    const inspection = page.getByRole('region',{name:'Applied source to destination activity'});
    await expect(inspection).not.toBeVisible();
    assert.equal(requests.filter(request=>request.command==='profile_runtime_topology').length,0);
    await activateByTab(page.getByRole('button',{name:'Debug',exact:true}),'Open Profile debug','Enter');
    const debug = page.getByRole('region',{name:'Profile debug'});
    await expect(debug.getByRole('heading',{name:'Profile debug',exact:true})).toBeVisible();
    await expect(inspection).toBeVisible();
    await expect.poll(()=>requests.filter(request=>request.command==='profile_runtime_topology').length).toBeGreaterThan(0);
    await activateByTab(debug.getByRole('button',{name:'← Back to Profiles',exact:true}),'Leave Profile debug','Space');
    await expect(inspection).not.toBeVisible();

    const add = page.getByRole('button',{name:'Add profile',exact:true});
    await activateByTab(add,'Add profile','Enter');
    await expect(page.getByRole('heading',{name:'New profile',exact:true})).toBeVisible();
    await expect(page.locator('.resource-block')).toHaveCount(0);
    await expect(page.getByRole('button',{name:'Add from inventory',exact:true})).toBeVisible();
    await page.getByLabel('Profile name',{exact:true}).fill('Browser rig');
    const save = page.getByRole('button',{name:'Save profile',exact:true});
    await activateByTab(save,'Save new profile','Enter');
    await expect(page.getByText('All changes saved · Save now and start this profile when ready.',{exact:true})).toBeVisible();
    await expect(runtime.getByRole('button',{name:'Start profile',exact:true})).toBeEnabled();
    const savedId = [...documents.keys()].find(id=>id!==defaultId);
    assert.ok(savedId);
    const back = page.getByRole('button',{name:'Back to Profiles',exact:false});
    await activateByTab(back,'Back from saved editor','Enter');
    const row = page.getByRole('button',{name:'Edit Browser rig',exact:true});
    await activateByTab(row,'Open exact saved profile','Enter');
    await expect(page.getByRole('heading',{name:'Browser rig',exact:true})).toBeVisible();
    await page.getByLabel('Profile name',{exact:true}).fill('Unsaved rig');
    await activateByTab(back,'Back with unsaved edits','Space');
    const dialog = page.getByRole('alertdialog',{name:'Keep your unsaved changes?'});
    await expect(dialog).toBeVisible();
    await activateByTab(dialog.getByRole('button',{name:'Keep editing',exact:true}),'Keep unsaved edits','Enter');
    await expect(dialog).not.toBeVisible();
    await expect(page.getByLabel('Profile name',{exact:true})).toHaveValue('Unsaved rig');
    await activateByTab(back,'Back before discard','Enter');
    await activateByTab(dialog.getByRole('button',{name:'Discard and continue',exact:true}),'Discard unsaved edits','Space');
    await expect(row).toBeVisible();
    assert.equal(documents.get(savedId).name,'Browser rig');

    await activateByTab(add,'Add cancelled draft','Space');
    await page.getByLabel('Profile name',{exact:true}).fill('Cancelled keyboard draft');
    await activateByTab(page.getByRole('button',{name:'Cancel creation',exact:true}),'Cancel new draft','Space');
    await expect(page.getByRole('list',{name:'Saved profiles'}).getByRole('listitem')).toHaveCount(2);
    assert.deepEqual(await page.getByRole('list',{name:'Saved profiles'}).getByRole('button',{name:/^Edit /}).evaluateAll(nodes=>nodes.map(node=>node.getAttribute('aria-label'))),['Edit Default','Edit Browser rig']);
    await expect(runtime).toContainText('Active profile · Default');
    assert.equal(requests.filter(request=>request.command==='profile_editor_save').length,1);

    // A separate dirty departure proves the modal Save choice, in addition to
    // keeping/discarding. This deliberately adds exactly one saved replacement.
    await activateByTab(row,'Reopen before guarded save','Space');
    await page.getByLabel('Profile name',{exact:true}).fill('Browser rig saved by keyboard');
    await activateByTab(back,'Back before guarded save','Space');
    await activateByTab(dialog.getByRole('button',{name:'Save and continue',exact:true}),'Save unsaved edits and leave','Enter');
    const savedRow = page.getByRole('button',{name:'Edit Browser rig saved by keyboard',exact:true});
    await expect(savedRow).toBeVisible();await expect(dialog).not.toBeVisible();
    assert.equal(documents.get(savedId).name,'Browser rig saved by keyboard');
    assert.equal(documents.get(savedId).revision,2);
    assert.deepEqual(requests.filter(request=>request.command==='profile_editor_save').map(request=>request.expectedRevision),[null,1]);
    assert.deepEqual(requests.filter(request=>request.command==='profile_assignments_get').map(request=>request.id),[defaultId,savedId,savedId]);
    await expect(runtime).toContainText('Active profile · Default');
    assert.ok(requests.filter(request=>request.command==='profile_runtime_topology').every(request=>JSON.stringify(request.owner)===JSON.stringify(owner)));
    assert.deepEqual(unexpected,[]);
    assert.deepEqual(errors,[]);
    assert.equal(documents.size,2);
    assert.equal(session.selectedProfileId,defaultId);
    assert.deepEqual(profileSession.owner,owner);
    assert.deepEqual(profileSession.appliedSnapshot,applied);
    assert.equal(documents.get(defaultId).revision,1);
    assert.equal(createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex'),driverSha256,'Driver source stayed unchanged during the browser proof');
    await page.screenshot({path:resolve(artifacts,'profiles-list.png'),fullPage:true});
    await writeFile(resolve(artifacts,'fixture-requests.json'),JSON.stringify(requests,null,2));
    await writeFile(resolve(artifacts,'keyboard-evidence.json'),JSON.stringify({result:'passed',recordedAt:new Date().toISOString(),driverSha256,browserVersion:browser.version(),browserChannel:process.env.OSG_BROWSER_CHANNEL??'chromium',scope:'Frontend-only actual browser; every native IPC intercepted, HTTP gateway fallback forbidden, no native profile effects.',keyboard,saveExpectedRevisions:[null,1],owner,appliedSnapshot:applied,selectedProfileId:session.selectedProfileId,unexpected,errors},null,2));
  } catch(error) {
    await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true}).catch(()=>{});
    await writeFile(resolve(artifacts,'failure.json'),JSON.stringify({error:error.stack,keyboard,requests,unexpected,errors,body:await page.locator('body').innerText().catch(()=> '')},null,2));
    throw error;
  } finally {await browser.close();}
});
