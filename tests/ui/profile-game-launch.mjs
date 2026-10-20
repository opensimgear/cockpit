// Frontend-only keyboard/navigation acceptance. Every native IPC is intercepted
// before navigation; the launched processes, profile owners and samples below
// are authored fixtures. This does not qualify native launch or hardware output.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const{chromium}=require(module),{expect}=require(module+'/test');
const endpoint=process.env.OSG_LAUNCH_UI_URL??'http://127.0.0.1:1420',artifacts=resolve(process.env.OSG_LAUNCH_UI_ARTIFACTS??'.research/profile-game-launch-browser');
const id='00000000-0000-4000-8000-000000000001',session='22222222-2222-4222-8222-222222222222',hash=`sha256:${'1'.repeat(64)}`;
const stopped=()=>({state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null});
const running=()=>({state:'running',appliedSnapshot:{profileId:id,revision:1,semanticHash:hash},owner:{session,generation:1,profileId:id,profileRevision:1,gameId:null},degradation:[],fault:null});
const deferred=()=>{let resolve;const promise=new Promise(yes=>resolve=yes);return{promise,resolve};};

test('keyboard launch confirms active-profile switches, deactivation wins late acknowledgement, and navigation never activates output',{timeout:60000},async()=>{
 await mkdir(artifacts,{recursive:true});const browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})}),page=await browser.newPage({viewport:{width:1440,height:1080}});
 const requests=[],errors=[],unexpected=[],holds=[];let launchMode='hold',nativeStatus=stopped();
 const catalogue=[{id:'dcs',name:'DCS World',provider:'dcs-bios',executable:'C:\\Games\\DCS\\bin\\DCS.exe'},{id:'x-plane-12',name:'X-Plane 12',provider:'x-plane',executable:'C:\\Games\\X-Plane 12\\X-Plane.exe'}].map(game=>({...game,installed:true,launchSupported:true,processMonitoringSupported:true,integrationSetup:{state:'setup-required',message:'Provider not set up in this browser fixture.',providerSetupRevision:hash,connection:game.id==='dcs'?{cataloguePath:'',freshnessMs:1500,commandHost:'127.0.0.1',commandPort:7778,listenAddress:'0.0.0.0',listenPort:5010,multicastGroup:'239.255.50.10',multicastInterface:'0.0.0.0'}:{source:'rref',version:12,host:'127.0.0.1',port:49000,listenPort:0,updateHz:10,freshnessMs:1500}},integrationContext:{status:'waiting-profile',message:'No real process or provider is authorized by a fixture.',processVerified:false,contextSession:null,providerSetupRevision:hash,catalogueRevision:null}}));
 const games={catalogue,cleanup:{phase:'idle'}};
 function profiles(status=nativeStatus){return{profiles:[{id,name:'Default',revision:1,isDefault:true,semanticHash:hash}],session:{selectedProfileId:id,selectionMode:'manual',state:'stopped',appliedSnapshot:null,fault:null},profileSession:status,automatic:{schemaVersion:1,mode:'manual',manualProfileId:id,rules:[],status:'manual',message:'Fixture Manual pin.',conflicts:[],lastAttempt:null,target:null,readOnly:false}};}
 function reply(extra={}){return{ok:true,state:{running:false,needsReconnect:false,modules:[],ports:[],games,integration:{running:false},dcsBios:{running:false},companions:{host:{instances:[],publications:[],clients:[]}},profiles:profiles(),...extra}};}
 page.on('pageerror',error=>errors.push(error.message));
 await page.route('**/api/gateway/**',async route=>{unexpected.push('Unexpected HTTP gateway fallback');await route.abort();});
 await page.exposeFunction('__osgLaunchFixture',async(command,args)=>{
  if(['profile_activation_window_open','profile_activation_window_update','profile_activation_window_close'].includes(command))return true;
  if(command==='profile_activation_window_state')return null;
  if(command!=='gateway_request'){unexpected.push(command);throw new Error('Only intercepted gateway requests belong to this fixture.');}
  const request=structuredClone(args.request);requests.push(request);
  if(request.command==='profile_game_candidates')return reply({profiles:{gameId:request.gameId,profiles:[],preferredProfileId:null}});
  if(request.command==='profile_game_launch'){
   assert.deepEqual(Object.keys(request).sort(),['clientId','command','expectedActiveOwner','gameId']);assert.deepEqual(request.expectedActiveOwner,nativeStatus.owner);const status=running();
   if(launchMode==='hold'){const hold=deferred();holds.push(hold);await hold.promise;}
   else{status.state='degraded';status.degradation=['Fixture optional companion is disconnected.'];nativeStatus=status;}
   return reply({profiles:{...profiles(status),launch:{gameId:request.gameId,profileId:id,selectedGameId:null,state:'started'}}});
  }
  if(request.command==='profile_stop'){nativeStatus=stopped();return reply();}
  return reply();
 });
 await page.addInitScript(()=>{window.isTauri=true;window.__TAURI_INTERNALS__={invoke:(command,args)=>window.__osgLaunchFixture(command,args)};});
 try{
  await page.goto(endpoint);const nav=page.getByRole('navigation',{name:'Main navigation'}),gamesNav=nav.getByRole('button',{name:'Games',exact:true});await gamesNav.focus();await page.keyboard.press('Enter');
  const dcs=page.locator('.game-tile').filter({has:page.getByRole('button',{name:'Open DCS World details',exact:true})}),tileOpen=dcs.getByRole('button',{name:'Open DCS World details',exact:true}),launch=dcs.getByRole('button',{name:'Launch DCS World',exact:true});
  await expect(launch).toBeEnabled();await expect(page.getByLabel('Select game profile')).toHaveCount(0);await expect(dcs.getByRole('button',{name:'Deactivate profile',exact:true})).toHaveCount(0);await expect(dcs.getByText('Unified profile selection',{exact:true})).toHaveCount(0);await tileOpen.focus();await page.keyboard.press('Tab');await expect(launch).toBeFocused();await page.keyboard.press('Space');
  await expect.poll(()=>holds.length).toBe(1);await expect(launch).toBeDisabled();await expect(dcs.getByText(/Preparing the saved profile/)).toBeVisible();await expect(dcs.getByText(/Game launch completed/)).toHaveCount(0);
  // Details retain cleanup controls. A former tile acknowledgement must not
  // change a new details component after its profile has been deactivated.
  await tileOpen.focus();await page.keyboard.press('Enter');const detail=page.locator('.game-details-heading'),detailLaunch=detail.getByRole('button',{name:'Launch DCS World',exact:true}),stop=detail.getByRole('button',{name:'Deactivate profile',exact:true});await expect(stop).toBeEnabled();await stop.focus();await page.keyboard.press('Space');await expect(detail.getByText('No profile active.',{exact:true})).toBeVisible();holds[0].resolve();await expect(detailLaunch).toBeEnabled();await expect(detail.getByText(/Game launch completed/)).toHaveCount(0);
  assert.equal(requests.filter(request=>request.command==='profile_game_launch').length,1);assert.equal(requests.filter(request=>request.command==='profile_stop').length,1);
  // Also fence a late acknowledgement within the same details component.
  await detailLaunch.focus();await page.keyboard.press('Enter');await expect.poll(()=>holds.length).toBe(2);await expect(stop).toBeEnabled();await stop.focus();await page.keyboard.press('Enter');await expect(detail.getByText('No profile active.',{exact:true})).toBeVisible();holds[1].resolve();await expect(detailLaunch).toBeEnabled();await expect(detail.getByText(/Game launch completed/)).toHaveCount(0);
  for(const obsolete of ['Captured mapping','Captured companions','Select game profile','Capture settings','Activate profile'])await expect(page.getByText(obsolete,{exact:true})).toHaveCount(0);
  await expect(page.getByRole('button',{name:'Launch DCS World',exact:true})).toBeEnabled();await expect(page.getByText('Provider not set up in this browser fixture.',{exact:true})).toBeVisible();
  launchMode='degraded';await detail.getByRole('button',{name:'Launch DCS World',exact:true}).focus();await page.keyboard.press('Enter');await expect(detail.getByText(/Game started with unavailable branches/)).toBeVisible();await expect(detail.getByText('Fixture optional companion is disconnected.',{exact:true})).toBeVisible();
  // Native ownership can change outside this view. Only a different profile
  // requires a switch question; launching the same Default profile does not.
  const otherProfile='33333333-3333-4333-8333-333333333333';nativeStatus={...nativeStatus,owner:{...nativeStatus.owner,profileId:otherProfile},appliedSnapshot:{...nativeStatus.appliedSnapshot,profileId:otherProfile}};
  const beforeSwitch=structuredClone(nativeStatus.owner),beforeLaunches=requests.filter(request=>request.command==='profile_game_launch').length;
  await detail.getByRole('button',{name:'Launch DCS World',exact:true}).click();
  const switchDialog=page.getByRole('alertdialog',{name:'Switch active profile?',exact:true});await expect(switchDialog).toBeVisible();await switchDialog.getByRole('button',{name:'Cancel',exact:true}).click();await expect(switchDialog).toHaveCount(0);
  assert.equal(requests.filter(request=>request.command==='profile_game_launch').length,beforeLaunches);assert.deepEqual(nativeStatus.owner,beforeSwitch);
  await detail.getByRole('button',{name:'Launch DCS World',exact:true}).click();await switchDialog.getByRole('button',{name:'Switch profile',exact:true}).click();await expect.poll(()=>requests.filter(request=>request.command==='profile_game_launch').length).toBe(beforeLaunches+1);assert.deepEqual(requests.filter(request=>request.command==='profile_game_launch').at(-1).expectedActiveOwner,beforeSwitch);
  await page.screenshot({path:resolve(artifacts,'games-unified-launch-desktop.png'),fullPage:true});
  const launches=requests.filter(request=>request.command==='profile_game_launch').length,back=page.getByRole('button',{name:'All games',exact:true});await back.focus();await page.keyboard.press('Enter');const xplane=page.getByRole('button',{name:'Open X-Plane 12 details',exact:true});await xplane.focus();await page.keyboard.press('Space');await expect(page.getByRole('heading',{name:'X-Plane 12',exact:true})).toBeVisible();assert.equal(requests.filter(request=>request.command==='profile_game_launch').length,launches,'Keyboard navigation must not launch a profile.');
  await page.setViewportSize({width:390,height:844});await expect(page.getByRole('button',{name:'Launch X-Plane 12',exact:true})).toBeVisible();await expect(page.getByRole('button',{name:'Deactivate profile',exact:true})).toBeVisible();await page.screenshot({path:resolve(artifacts,'games-unified-launch-mobile.png'),fullPage:true});
  const dimensions=await page.locator('main').evaluate(node=>({width:node.clientWidth,content:node.scrollWidth}));assert.ok(dimensions.content<=dimensions.width+1,JSON.stringify(dimensions));
  assert.equal(requests.some(request=>['game_launch','game_activate','game_profile_save','game_profile_default','mapping_preview_start','mapping_virtual_start','integration_start','dcs_start','profile_apply','profile_select','profile_selection_mode'].includes(request.command)),false);
  assert.deepEqual(unexpected,[]);assert.deepEqual(errors,[]);await writeFile(resolve(artifacts,'fixture-requests.json'),JSON.stringify(requests,null,2));console.log(`Frontend-only keyboard launch fixture passed; ${requests.length} intercepted IPCs. Artifacts: ${artifacts}`);
 }catch(error){console.log(await page.locator('main').innerText());console.log(JSON.stringify({requests,errors,unexpected},null,2));throw error;}
 finally{holds.forEach(hold=>hold.resolve());await browser.close();}
});
