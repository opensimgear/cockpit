// Opt-in acceptance against the actual running native dev Cockpit. Authored
// shared-memory publishers exercise native acquisition without qualifying a game build.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {access,mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';

const enabled=process.env.OSG_ASSETTO_NATIVE==='1',require=createRequire(import.meta.url);
const execute=promisify(execFile),sleep=ms=>new Promise(r=>setTimeout(r,ms));
const specs=[['assetto-corsa','Assetto Corsa','acs.exe'],['assetto-corsa-competizione','Assetto Corsa Competizione','AC2-Win64-Shipping.exe'],['assetto-corsa-evo','Assetto Corsa EVO','AssettoCorsaEVO.exe']];

test('seven game artworks and three native Assetto dialects work in the running dev cockpit',{skip:!enabled,timeout:240000},async t=>{
  assert.equal(process.platform,'win32');
  const modulePath=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(modulePath),{expect}=require(modulePath+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222',{timeout:15000});
  t.after(()=>browser.close());
  const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes(':1420/')&&!p.url().includes('profile-activation-window'));
  assert.ok(page,'Start pnpm dev with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222.');
  page.setDefaultTimeout(15000);
  const artifacts=resolve('.research/assetto-native',String(Date.now()));await mkdir(artifacts,{recursive:true});
  const evidence={actualNativeCockpit:true,authoredNativePublishers:true,observedAt:new Date().toISOString(),games:[],checks:[]};
  const native=async request=>{
    const response=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'assetto-native-acceptance'}}),request);
    assert.equal(response.ok,true,`${request.command}: ${response.error}`);return response.state;
  };
  const catalogue=async()=>(await native({command:'game_get_state'})).games.catalogue;
  const wait=async(fn,label,timeout=25000)=>{const until=Date.now()+timeout;while(Date.now()<until){const result=await fn();if(result)return result;await sleep(250);}throw Error(label);};
  const initialState=(await native({command:'game_get_state'})).games,initial=initialState.catalogue;
  assert.equal(initial.length,7);assert.equal(new Set(initial.map(g=>g.id)).size,7);
  assert.ok(!initialState.detectedGames?.some(g=>g.gameId.startsWith('assetto-corsa')),'Close real Assetto games before publishing authored test pages.');
  await native({command:'profile_selection_mode',mode:'manual'});
  await native({command:'profile_stop'});
  evidence.installations=initial.map(g=>({id:g.id,installed:g.installed,executable:g.executable}));
  await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();
  const back=page.getByRole('button',{name:'All games',exact:true});if(await back.isVisible())await back.click();
  await page.getByRole('switch',{name:'Show all supported',exact:true}).check();
  await expect(page.locator('.game-tile')).toHaveCount(7);
  for(const image of await page.locator('.game-art-image').all()){
    await image.scrollIntoViewIfNeeded();await expect.poll(()=>image.evaluate(img=>img.complete&&img.naturalWidth>0)).toBe(true);
    assert.match(await image.getAttribute('src'),/^\/game-art\//);
  }
  const cdp=await page.context().newCDPSession(page);
  try{
    for(const width of [1440,390]){
      await cdp.send('Emulation.setDeviceMetricsOverride',{width,height:1080,deviceScaleFactor:1,mobile:false});
      await page.locator('main').evaluate(main=>main.scrollTo(0,0));
      await page.screenshot({path:resolve(artifacts,`games-${width}.png`),fullPage:true});
      assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'Game tiles must fit the viewport.');
    }
  }finally{await cdp.send('Emulation.clearDeviceMetricsOverride');await cdp.detach();}
  evidence.checks.push('Seven local images load; game grid fits 1440 and 390 pixel viewports.');
  for(const [gameId,name,basename] of specs){
    const directory=resolve(artifacts,gameId);await mkdir(directory,{recursive:true});
    if(gameId==='assetto-corsa'){
      for(const [id,label] of [['osg_fixture_car','Authored Live Car'],['osg_installed_mod','Authored Installed Mod']]){
        const car=resolve(directory,'content/cars',id);
        await mkdir(resolve(car,'ui'),{recursive:true});await mkdir(resolve(car,'skins/default'),{recursive:true});
        await writeFile(resolve(car,'ui/ui_car.json'),JSON.stringify({name:label}));
        await writeFile(resolve(car,'data.acd'),'Authored archive presence fixture');
        await writeFile(resolve(car,'skins/default/livery.dds'),'Authored skin presence fixture');
      }
    }
    const executable=resolve(directory,basename),stopfile=resolve(directory,'stop');
    await execute('C:\\Windows\\Microsoft.NET\\Framework64\\v4.0.30319\\csc.exe',['/nologo','/target:winexe',`/out:${executable}`,resolve('scripts/fixtures/assetto-telemetry/Program.cs')],{windowsHide:true});
    const previous=(await native({command:'game_get_state'})).games.manualInstallations?.[gameId]??null;
    let child,profile,exited;
    async function stopFixture(){
      if(!child)return;
      const owned=child;
      await writeFile(stopfile,'stop');
      const timer=setTimeout(()=>owned.kill(),5000);
      try{await exited;}finally{clearTimeout(timer);child=null;}
    }
    try{
      await native({command:'game_installation',gameId,executable});
      child=spawn(executable,[gameId,stopfile],{windowsHide:true,stdio:'ignore'});
      exited=new Promise(resolve=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',error=>resolve({error:String(error)}));});
      await new Promise((r,j)=>{child.once('spawn',r);child.once('error',j);});
      await wait(async()=>{
        if(child.exitCode!==null||child.signalCode!==null)throw Error(`Telemetry publisher exited: ${await readFile(stopfile+'.error','utf8').catch(()=>String(child.exitCode))}`);
        return access(stopfile+'.ready').then(()=>true,()=>false);
      },`${name} publisher did not initialize.`);
      await native({command:'game_integration_setup',gameId});
      const game=await wait(async()=>{const g=(await catalogue()).find(g=>g.id===gameId);return g.integrationSetup?.state!=='setting-up'&&g;},`${name} setup did not complete.`);
      assert.equal(game.integrationSetup.verified,true,JSON.stringify(game.integrationSetup));
      assert.equal(game.integrationSetup.state,'ready');
      assert.equal(game.integrationSetup.connection.source,'shared-memory');
      await native({command:'game_integration_check',gameId});
      const live=await wait(async()=>{const g=(await catalogue()).find(g=>g.id===gameId);return g.currentVehicle?.id==='osg_fixture_car'&&g;},`${name} did not discover the native car.`);
      if(gameId==='assetto-corsa'){
        assert.deepEqual(live.vehicleSupport.vehicles,[{id:'osg_fixture_car',name:'Authored Live Car'},{id:'osg_installed_mod',name:'Authored Installed Mod'}]);
        assert.equal(live.currentVehicle.name,'Authored Live Car');
        evidence.checks.push('Original AC setup lists both installed cars with metadata names, including a mod that was never driven.');
      }
      evidence.games.push({gameId,setup:game.integrationSetup,vehicle:live.currentVehicle});
      const resources=(await native({command:'profile_resources_list',offset:0,limit:128})).profileResources.resources;
      const resource=resources.find(r=>r.attributes.gameId===gameId);assert.ok(resource,`${name} profile integration resource missing.`);
      const ports=(await native({command:'profile_resources_inspect',resourceId:resource.id,offset:0,limit:128})).profileResources.ports;
      assert.equal(ports.length,5);assert.ok(ports.every(p=>p.descriptor.direction==='output'));
      assert.deepEqual(ports.map(p=>p.descriptor.id).sort(),['channel:gear','channel:max_rpm','channel:rev_ratio','channel:rpm','channel:speed_kmh']);
      const document=(await native({command:'profile_create',name:`Native ${name} ${Date.now()}`})).profiles.document;
      profile={id:document.id,revision:document.revision};
      document.eligibility={gameIds:[gameId]};
      document.resources.push({id:'telemetry',name:'Native telemetry',kind:'game',required:true,ports:ports.map(p=>({...p.descriptor,id:p.descriptor.id.replace('channel:','')}))});
      for(const p of ports){
        const id=p.descriptor.id.replace('channel:',''),contract=p.descriptor.contract;
        document.processingNodes.push({id:`value-${id}`,name:`Observed ${id}`,operation:{kind:'identity'},inputs:[{id:'in',name:'Input',direction:'input',contract}],outputs:[{id:'out',name:'Output',direction:'output',contract}]});
        document.bindings.push({id:`route-${id}`,from:{nodeId:'telemetry',portId:id},to:{nodeId:`value-${id}`,portId:'in'}});
      }
      const assignments=[{resourceId:'telemetry',sourceId:resource.id,ports:ports.map(p=>({profilePortId:p.descriptor.id.replace('channel:',''),observedPortId:p.descriptor.id,expectedContractHash:p.contractHash}))}];
      const saved=(await native({command:'profile_editor_save',document,assignments,expectedRevision:document.revision})).profiles.document;
      profile.revision=saved.revision;
      await native({command:'profile_apply',id:profile.id,expectedRevision:saved.revision,gameId,expectedActiveOwner:null});
      const expected={rpm:4500,gear:2,speed_kmh:123.5,max_rpm:9000,rev_ratio:0.5};
      const samples=await wait(async()=>{
        const status=(await native({command:'profile_status'})).profiles.profileSession;
        if(status.owner?.profileId!==profile.id)return false;
        const inspected=(await native({command:'profile_runtime_inspect',owner:status.owner,offset:0,limit:64})).profileInspection;
        return Object.entries(expected).every(([id,value])=>inspected.ports.some(p=>p.endpoint.nodeId===`value-${id}`&&p.endpoint.portId==='out'&&p.sample.status==='known'&&p.sample.value.value===value))&&inspected;
      },`${name} did not deliver the native channels through its applied profile.`);
      evidence.games.at(-1).inspection=samples;
      await page.getByRole('button',{name:new RegExp(`^Open ${name} details`)}).click();
      await expect(page.getByRole('heading',{name,exact:true})).toBeVisible();
      await expect(page.getByRole('region',{name:`${name} provider settings`})).toBeVisible();
      await expect(page.getByRole('region',{name:'Game integration setup'}).getByText('Integration ready',{exact:true})).toBeVisible();
      await page.screenshot({path:resolve(artifacts,`${gameId}-details.png`),fullPage:true});
      await page.getByRole('button',{name:'All games',exact:true}).click();
      await stopFixture();
      await wait(async()=>!(await catalogue()).find(g=>g.id===gameId).currentVehicle,`${name} kept a vehicle after process exit.`);
      await native({command:'profile_stop'});
      await native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});profile=null;
      evidence.checks.push(`${name}: native process, verified setup, live car, five read-only ports and exact live values through an applied profile; process exit clears vehicle.`);
    }finally{
      await stopFixture();
      if(profile){await native({command:'profile_stop'});await native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});}
      await native({command:'game_installation',gameId,executable:previous});
    }
  }
  await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2));
  console.log(JSON.stringify({artifacts,checks:evidence.checks}));
});
