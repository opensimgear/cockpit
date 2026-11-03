// Authored native publishers in the running Cockpit; no real game or outputs.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,writeFile,readFile,access,rm} from 'node:fs/promises';
import {resolve,sep} from 'node:path';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

const specs=[['assetto-corsa','acs.exe'],['assetto-corsa-competizione','AC2-Win64-Shipping.exe'],
  ['assetto-corsa-evo','AssettoCorsaEVO.exe'],['assetto-corsa-rally','acr.exe']];
const execute=promisify(execFile);
const normalized=path=>path.replace(/^\\\\\?\\/,'').replaceAll('\\','/').toLowerCase();

test('four Assetto native page readers preserve processing, frozen expiry and recovery',{
  skip:process.env.OSG_ASSETTO_CAPTURE_NATIVE!=='1',timeout:300000,
},async()=>{
  assert.equal(process.platform,'win32');
  const page=await nativePage(),directory=resolve('.research/assetto-capture-native',String(Date.now()));
  await mkdir(directory,{recursive:true});
  const evidence={actualNativeCockpit:true,authoredNativePublishers:true,actualSimulatorQualified:false,
    noPhysicalOutputs:true,startedAt:new Date().toISOString(),games:[],cleanupErrors:[]};
  const native=async request=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...request,clientId:'assetto-capture-acceptance'})}})`);
    assert.equal(reply.ok,true,`${request.command}: ${reply.error}`);return reply.state;
  };
  const games=async()=>(await native({command:'game_get_state'})).games;
  try{
    const initial=await games();
    assert.ok(!initial.detectedGames?.some(game=>game.gameId.startsWith('assetto-corsa')),
      'Close real Assetto games before starting private publishers.');
    await native({command:'profile_selection_mode',mode:'manual'});
    await native({command:'profile_stop'});
    for(const [gameId,basename] of specs){
      const working=resolve(directory,gameId);await mkdir(working,{recursive:true});
      if(gameId==='assetto-corsa'){
        const car=resolve(working,'content/cars/osg_expanded_car');
        await mkdir(resolve(car,'ui'),{recursive:true});await mkdir(resolve(car,'skins/default'),{recursive:true});
        await writeFile(resolve(car,'ui/ui_car.json'),JSON.stringify({name:'Authored capture car'}));
        await writeFile(resolve(car,'data.acd'),'Authored presence fixture');
        await writeFile(resolve(car,'skins/default/livery.dds'),'Authored presence fixture');
      }
      const executable=resolve(working,basename),stop=resolve(working,'stop');
      await execute('C:\\Windows\\Microsoft.NET\\Framework64\\v4.0.30319\\csc.exe',[
        '/nologo','/target:winexe',`/out:${executable}`,resolve('scripts/fixtures/assetto-telemetry/ExpandedProgram.cs'),
      ],{windowsHide:true});
      const previous=(await games()).catalogue.find(game=>game.id===gameId);
      assert.ok(previous);
      const record={gameId,phases:{}};evidence.games.push(record);
      let child,exited,profile,installation;
      const stopPublisher=async()=>{
        if(!child)return;const owned=child;await writeFile(stop,'stop');
        const timer=setTimeout(()=>owned.kill(),5000);
        try{await exited;}finally{clearTimeout(timer);child=null;}
      };
      try{
        const registered=(await native({command:'game_add',name:previous.name,installPath:working,executable})).games;
        installation=registered.catalogue.find(game=>game.id===gameId)?.installations.find(item=>normalized(item.installPath)===normalized(working));
        assert.ok(installation,`${gameId}: private installation registration`);
        await native({command:'game_installation_select',gameId,installationId:installation.id});
        child=spawn(executable,[gameId,stop],{windowsHide:true,stdio:'ignore'});
        exited=new Promise(resolve=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',error=>resolve({error:String(error)}));});
        await new Promise((resolve,reject)=>{child.once('spawn',resolve);child.once('error',reject);});
        await until(async()=>access(stop+'.ready').then(()=>true,()=>false),Boolean,15000);
        await native({command:'game_integration_setup',gameId,connection:{freshnessMs:500}});
        const ready=await until(async()=>{
          const game=(await games()).catalogue.find(game=>game.id===gameId);
          return game.integrationSetup?.state!=='setting-up'&&game;
        },Boolean,25000);
        assert.equal(ready.integrationSetup.verified,true,JSON.stringify(ready.integrationSetup));
        const resources=(await native({command:'profile_resources_list',offset:0,limit:128})).profileResources.resources;
        const resource=resources.find(resource=>resource.attributes.gameId===gameId);assert.ok(resource);
        const allPorts=[];let total=1;
        while(allPorts.length<total){
          const result=(await native({command:'profile_resources_inspect',resourceId:resource.id,offset:allPorts.length,limit:128})).profileResources;
          total=result.total;assert.ok(result.ports.length||allPorts.length===total);allPorts.push(...result.ports);
        }
        const expected={rpm:4500,max_rpm:9000,rev_ratio:0.5,'engine.rpm':4500,'vehicle.speed_mps':20};
        const selected=Object.keys(expected).map(id=>{const port=allPorts.find(port=>port.descriptor.id===`channel:${id}`);assert.ok(port,`${gameId}: ${id}`);return port;});
        const document=(await native({command:'profile_create',name:`Capture ${gameId} ${Date.now()}`})).profiles.document;
        profile={id:document.id,revision:document.revision};record.profile=profile;
        document.eligibility={gameIds:[gameId]};
        document.resources.push({id:'telemetry',name:'Authored capture',kind:'game',required:true,
          ports:selected.map(port=>({...port.descriptor,id:port.descriptor.id.slice(8)}))});
        for(const port of selected){
          const id=port.descriptor.id.slice(8),contract=port.descriptor.contract;
          document.processingNodes.push({id:`value-${id}`,name:id,operation:{kind:'identity'},
            inputs:[{id:'in',name:'Input',direction:'input',contract}],outputs:[{id:'out',name:'Output',direction:'output',contract}]});
          document.bindings.push({id:`route-${id}`,from:{nodeId:'telemetry',portId:id},to:{nodeId:`value-${id}`,portId:'in'}});
        }
        const assignments=[{resourceId:'telemetry',sourceId:resource.id,ports:selected.map(port=>({
          profilePortId:port.descriptor.id.slice(8),observedPortId:port.descriptor.id,expectedContractHash:port.contractHash,
        }))}];
        const saved=(await native({command:'profile_editor_save',document,assignments,expectedRevision:profile.revision})).profiles.document;
        profile.revision=saved.revision;
        await native({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,gameId,expectedActiveOwner:null});
        const samples=async()=>{
          const session=(await native({command:'profile_status'})).profiles.profileSession;
          assert.equal(session.owner?.profileId,profile.id);
          return (await native({command:'profile_runtime_inspect',owner:session.owner,offset:0,limit:64})).profileInspection;
        };
        const live=()=>until(async()=>{
          const snapshot=await samples();return Object.entries(expected).every(([id,value])=>snapshot.ports.some(port=>
            port.endpoint.nodeId===`value-${id}`&&port.endpoint.portId==='out'&&port.sample.status==='known'&&Math.abs(port.sample.value.value-value)<1e-5))&&snapshot;
        },Boolean,25000);
        const mode=async value=>{await writeFile(stop+'.mode',value);await until(()=>readFile(stop+'.ack','utf8').then(actual=>actual===value,()=>false));};
        record.phases.live=await live();
        await mode('frozen');
        record.phases.frozen=await until(async()=>{
          const snapshot=await samples();return Object.keys(expected).every(id=>snapshot.ports.some(port=>
            port.endpoint.nodeId===`value-${id}`&&port.endpoint.portId==='out'&&port.sample.status!=='known'))&&snapshot;
        },Boolean,25000);
        await mode('live');record.phases.recovered=await live();
        console.log(`${gameId}: native processing, frozen expiry and recovery pass`);
      }finally{
        const cleanup=async(label,fn)=>{try{await fn();}catch(error){evidence.cleanupErrors.push(`${gameId} ${label}: ${error}`);}};
        await cleanup('publisher',stopPublisher);
        if(profile){await cleanup('stop',()=>native({command:'profile_stop'}));await cleanup('delete',()=>native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision}));}
        if(installation)await cleanup('installation',async()=>{
          if(previous.selectedInstallationId)await native({command:'game_installation_select',gameId,installationId:previous.selectedInstallationId});
          assert.ok(directory.startsWith(resolve('.research/assetto-capture-native')+sep));
          assert.ok(working.startsWith(directory+sep));
          assert.equal(normalized(installation.installPath),normalized(working));
          await rm(working,{recursive:true,force:true});
          await until(async()=>(await games()).discovery?.status!=='scanning',Boolean,90000);
          const scan=async paths=>{
            await native({command:'game_discovery_start',paths});
            return until(async()=>{const state=(await games()).discovery;return state.status==='ready'&&state;},Boolean,90000);
          };
          let discovery=await scan([]);
          if(!discovery.libraries.some(library=>library.id==='disk'))discovery=await scan([directory]);
          const removal=discovery.entries.find(item=>item.action==='remove'&&item.gameId===gameId&&item.installationId===installation.id);
          assert.ok(removal,'Discovery must offer removal of only the private installation');
          await native({command:'game_discovery_apply',scanId:discovery.scanId,actions:discovery.entries.map(item=>({id:item.id,selected:item.id===removal.id}))});
          const restored=(await games()).catalogue.find(game=>game.id===gameId);
          assert.deepEqual(restored.installations,previous.installations);
          record.installationRestored=true;
        });
      }
    }
    assert.deepEqual(evidence.cleanupErrors,[]);
    evidence.finishedAt=new Date().toISOString();await writeFile(resolve(directory,'evidence.json'),JSON.stringify(evidence,null,2));
  }catch(error){await writeFile(resolve(directory,'failure.json'),JSON.stringify({...evidence,error:String(error)},null,2));throw error;}
  finally{page.close();}
});
