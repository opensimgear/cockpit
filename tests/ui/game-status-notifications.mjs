// Every IPC is intercepted before navigation; no native hardware or game effect.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(module),{expect}=require(module+'/test');
const endpoint=process.env.OSG_SETUP_UI_URL??'http://127.0.0.1:1420';
const missingCleanup='Native cleanup metadata is unavailable. Refresh game status before changing provider setup.';
const incomplete='The request did not complete. Check the connection and try again.';
const scanFailure='Fixture rescan failed: installation discovery unavailable.';
const id='00000000-0000-4000-8000-000000000001',hash=`sha256:${'1'.repeat(64)}`;

async function fixture({blockInitial=false,omitCleanup=false,failScan=false}={}) {
  const browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
  const page=await browser.newPage({viewport:{width:1440,height:1080}}),requests=[],unexpected=[],errors=[];
  const game={id:'x-plane-12',name:'X-Plane 12',provider:'x-plane',installed:true,executable:'C:\\Games\\X-Plane 12\\X-Plane.exe',launchSupported:true,processMonitoringSupported:true,
    integrationSetup:{state:'ready',message:'Fixture integration ready.',providerSetupRevision:hash,connection:{source:'rref',version:12,host:'127.0.0.1',port:49000,listenPort:0,updateHz:10,freshnessMs:1500},live:'waiting'},
    integrationContext:{status:'waiting-profile',message:'No active profile in this fixture.',processVerified:false,contextSession:null,providerSetupRevision:hash,catalogueRevision:null}};
  const games={catalogue:[game],...(!omitCleanup?{cleanup:{phase:'idle'}}:{}),detectedGames:[],detectionErrors:[],manualInstallations:{}};
  const profiles={profiles:[{id,name:'Default',revision:1,isDefault:true,semanticHash:hash}],profileSession:{state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null},automatic:{schemaVersion:1,mode:'manual',manualProfileId:id,rules:[],status:'manual',message:'Fixture Manual mode.',conflicts:[],lastAttempt:null,target:null,readOnly:false}};
  const state={running:false,needsReconnect:false,modules:[],ports:[],games,profiles,companions:{host:{instances:[],clients:[],publications:[]}},retiredRecovery:{required:false,outputs:[],provisioning:[],job:null,diagnostics:[],outputQualified:false,provisioningQualified:false}};
  let releaseInitial;const initial=new Promise(resolve=>{releaseInitial=resolve;});
  page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/api/gateway/**',async route=>{unexpected.push('HTTP fallback');await route.abort();});
  await page.exposeFunction('__osgGameStatusFixture',async(command,args)=>{
    if(command!=='gateway_request'){unexpected.push(command);throw Error('Unexpected non-gateway IPC.');}
    const request=structuredClone(args.request);requests.push(request);
    if(request.command==='game_get_state'&&blockInitial&&requests.filter(r=>r.command==='game_get_state').length===1)await initial;
    if(request.command==='profile_game_candidates')return{ok:true,state:{profiles:{gameId:request.gameId,profiles:[],preferredProfileId:null}}};
    if(request.command==='game_scan'&&failScan)return{ok:false,error:scanFailure};
    if(!['profile_status','profile_list','profile_get','profile_assignments_get','profile_resources_list','profile_desktop_status','profile_automatic_status','profile_activation_current','profile_retired_recovery_status','controller_list','module_usb_list','system_inventory','ports','status','companion_status','game_get_state','game_scan'].includes(request.command)){
      unexpected.push(request.command);throw Error('Unexpected effect or retired route '+request.command);
    }
    return{ok:true,state};
  });
  await page.addInitScript(()=>{
    window.isTauri=true;
    window.__TAURI_INTERNALS__={invoke:(command,args)=>window.__osgGameStatusFixture(command,args)};
    // Capture transient notifications too, including ones dismissed by rerenders.
    window.__osgObservedNotifications=[];
    new MutationObserver(()=>{
      for(const p of document.querySelectorAll('[aria-label="Notifications"] .toast p'))window.__osgObservedNotifications.push(p.textContent);
    }).observe(document,{childList:true,subtree:true});
  });
  await page.goto(endpoint);
  const nav=page.getByRole('navigation',{name:'Main navigation'}),notifications=page.getByRole('region',{name:'Notifications'});
  return{browser,page,nav,notifications,requests,errors,unexpected,releaseInitial,
    async openGames(){await nav.getByRole('button',{name:'Games',exact:true}).click();},
    async assertClean(){assert.deepEqual(unexpected,[]);assert.deepEqual(errors,[]);},
    async assertNoFalseNotifications(){const seen=await page.evaluate(()=>window.__osgObservedNotifications);assert.equal(seen.includes(missingCleanup),false);assert.equal(seen.includes(incomplete),false);await expect(notifications.locator('.toast')).toHaveCount(0);}};
}

test('Games loading and repeated remounts do not turn a coalesced status read into failure notifications',{timeout:30000},async()=>{
  const h=await fixture({blockInitial:true});
  try{
    await expect.poll(()=>h.requests.filter(r=>r.command==='game_get_state').length).toBe(1);
    for(let i=0;i<3;i++){
      await h.openGames();await expect(h.page.getByRole('heading',{name:'Games',exact:true})).toBeVisible();
      await expect(h.page.getByText('Loading game status…',{exact:true})).toBeVisible();
      // A passive read must not lock the user's controls while the first read is pending.
      await expect(h.page.getByRole('button',{name:'Rescan',exact:true})).toBeEnabled();
      await h.assertNoFalseNotifications();
      await h.nav.getByRole('button',{name:'Activity',exact:true}).click();
    }
    assert.equal(h.requests.filter(r=>r.command==='game_get_state').length,1,'mounts share the still-pending native status read');
    await h.openGames();
    await h.page.getByRole('switch',{name:'Show all supported',exact:true}).check();
    await h.page.getByRole('button',{name:'Open X-Plane 12 details, installation not found',exact:true}).click();
    await expect(h.page.getByRole('region',{name:'Game integration setup'}).getByRole('button',{name:'Install integration',exact:true})).toBeDisabled();
    await h.assertNoFalseNotifications();await h.page.getByRole('button',{name:'All games',exact:true}).click();
    h.releaseInitial();
    await expect(h.page.getByRole('button',{name:'Open X-Plane 12 details',exact:true})).toBeVisible();
    await h.assertNoFalseNotifications();await h.assertClean();
  }finally{h.releaseInitial();await h.browser.close();}
});

test('a received native status with missing cleanup still warns and blocks provider setup',{timeout:30000},async()=>{
  const h=await fixture({omitCleanup:true});
  try{
    await h.openGames();await h.page.getByRole('button',{name:'Open X-Plane 12 details',exact:true}).click();
    await expect(h.notifications.getByText(missingCleanup,{exact:true})).toBeVisible();
    await expect(h.page.getByRole('region',{name:'Game integration setup'}).getByRole('button',{name:'Repair integration',exact:true})).toBeDisabled();
    await expect(h.notifications.locator('.toast')).toHaveCount(1);await h.assertClean();
  }finally{await h.browser.close();}
});

test('explicit Rescan failure stays visible while later passive status reads recover',{timeout:30000},async()=>{
  const h=await fixture({failScan:true});
  try{
    await h.openGames();await expect(h.page.getByRole('button',{name:'Open X-Plane 12 details',exact:true})).toBeVisible();
    await h.page.getByRole('button',{name:'Rescan',exact:true}).click();
    await expect(h.notifications.getByText(scanFailure,{exact:true})).toBeVisible();
    await expect(h.notifications.locator('.toast')).toHaveCount(1);
    await expect(h.notifications.locator('.toast.error')).toHaveCount(1);
    await expect(h.notifications.getByText(incomplete,{exact:true})).toHaveCount(0);
    const reads=h.requests.filter(r=>r.command==='game_get_state').length;
    await expect.poll(()=>h.requests.filter(r=>r.command==='game_get_state').length).toBeGreaterThan(reads);
    await expect(h.notifications.getByText(scanFailure,{exact:true})).toBeVisible();
    await expect(h.notifications.locator('.toast')).toHaveCount(1);
    await expect(h.page.getByRole('button',{name:'Rescan',exact:true})).toBeEnabled();await h.assertClean();
  }finally{await h.browser.close();}
});
