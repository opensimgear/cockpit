// Browser acceptance with authored gateway replies. No native service, device,
// driver or game is used. Start a frontend-only Vite server before running.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

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
  const browser = await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL ? {channel:process.env.OSG_BROWSER_CHANNEL} : {})});
  const page = await browser.newPage({viewport:{width:1440,height:1100}});
  const documents = new Map([[defaultId, blank()]]), requests = [], unexpected = [], errors = [];
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
    if(request.command==='profile_resources_list')return {ok:true,state:{profileResources:{session:'fixture-inventory',revision:'1',offset:0,total:0,resources:[],inventoryDiagnostics:[],readOnly:false}}};
    if(request.command==='profile_assignments_get')return {ok:true,state:{profiles:{...plain(listing()),document:plain(documents.get(request.id)),assignments:[]}}};
    if(request.command==='profile_editor_save'){
      assert.equal(request.expectedRevision,documents.get(request.document.id)?.revision??null);
      const document={...plain(request.document),revision:request.expectedRevision===null?1:request.expectedRevision+1};
      documents.set(document.id,document);
      return {ok:true,state:{profiles:{...plain(listing()),document:plain(document),assignments:plain(request.assignments)}}};
    }
    if(request.command==='profile_runtime_topology')return {ok:true,state:{profileTopology:{schemaVersion:1,owner,appliedSnapshot:applied,programHash:hash,capturedUs:'0',section:request.section,offset:0,total:0,items:[]}}};
    if(request.command==='profile_runtime_inspect')return {ok:true,state:{profileInspection:{schemaVersion:1,owner,appliedSnapshot:applied,programHash:hash,capturedUs:'0',offset:0,total:0,ports:[]}}};
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
    await expect(runtime).toContainText('Default · 1');
    const inspection = page.getByRole('region',{name:'Applied source to destination activity'});
    await expect(inspection).not.toBeVisible();
    assert.equal(requests.filter(request=>request.command==='profile_runtime_topology').length,0);
    const tools = page.locator('details.runtime-tools > summary');
    await tools.click();
    await expect(inspection).toBeVisible();
    await expect.poll(()=>requests.filter(request=>request.command==='profile_runtime_topology').length).toBeGreaterThan(0);
    await tools.click();
    await expect(inspection).not.toBeVisible();

    const add = page.getByRole('button',{name:'Add profile',exact:true});
    await add.focus();await page.keyboard.press('Enter');
    await expect(page.getByRole('heading',{name:'New profile',exact:true})).toBeVisible();
    await page.getByLabel('Profile name',{exact:true}).fill('Browser rig');
    const save = page.getByRole('button',{name:'Save profile',exact:true});
    await save.focus();await page.keyboard.press('Enter');
    await expect(page.getByText('Saved revision 1 · Editing this profile does not activate it.',{exact:true})).toBeVisible();
    const savedId = [...documents.keys()].find(id=>id!==defaultId);
    assert.ok(savedId);
    await page.getByRole('button',{name:'Back to Profiles',exact:false}).click();
    const row = page.getByRole('button',{name:'Edit Browser rig',exact:true});
    await row.focus();await page.keyboard.press('Enter');
    await expect(page.getByRole('heading',{name:'Browser rig',exact:true})).toBeVisible();
    await page.getByLabel('Profile name',{exact:true}).fill('Unsaved rig');
    await page.getByRole('button',{name:'Back to Profiles',exact:false}).click();
    const dialog = page.getByRole('alertdialog',{name:'Keep your unsaved changes?'});
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button',{name:'Keep editing',exact:true}).click();
    await expect(page.getByLabel('Profile name',{exact:true})).toHaveValue('Unsaved rig');
    await page.getByRole('button',{name:'Back to Profiles',exact:false}).click();
    await dialog.getByRole('button',{name:'Discard and continue',exact:true}).click();
    await expect(row).toBeVisible();
    assert.equal(documents.get(savedId).name,'Browser rig');

    await add.click();
    await page.getByRole('button',{name:'Cancel creation',exact:true}).click();
    await expect(page.getByRole('list',{name:'Saved profiles'}).getByRole('button')).toHaveCount(2);
    await expect(runtime).toContainText('Default · 1');
    assert.equal(requests.filter(request=>request.command==='profile_editor_save').length,1);
    assert.ok(requests.filter(request=>request.command==='profile_runtime_topology').every(request=>JSON.stringify(request.owner)===JSON.stringify(owner)));
    assert.deepEqual(unexpected,[]);
    assert.deepEqual(errors,[]);
    assert.equal(documents.size,2);
    assert.equal(session.selectedProfileId,defaultId);
    assert.deepEqual(profileSession.owner,owner);
    await mkdir(artifacts,{recursive:true});
    await page.screenshot({path:resolve(artifacts,'profiles-list.png'),fullPage:true});
    await writeFile(resolve(artifacts,'fixture-requests.json'),JSON.stringify(requests,null,2));
  } finally {await browser.close();}
});
