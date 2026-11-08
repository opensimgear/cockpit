import assert from 'node:assert/strict';
import test from 'node:test';
import {render} from 'svelte/server';
import {loadProcessMonitor,processMonitorModules} from './process-monitor-ui-loader.mjs';

const modules=await processMonitorModules();
const games=modules.get('./games');
const xplane=(processMonitor={state:'waiting',message:'Start X-Plane to prepare monitoring.'},overrides={})=>({id:'x-plane-12',name:'X-Plane 12',provider:'x-plane',installed:true,launchSupported:true,processMonitoringSupported:true,processMonitor,...overrides});
async function setup(game=xplane(),request=async()=>{}){
  games.updateGameCatalogue([game]);
  const built=await loadProcessMonitor(modules,request,true);
  const html=render(built.exports.default,{props:{gameId:game.id}}).body;
  return {...built,html};
}

test('waiting and permission states render an explicit Allow action without requesting or elevating',async()=>{
  let calls=0;const {html,instrumented}=await setup(xplane({state:'needs-permission',message:'Permission is required.'}),async()=>calls++);
  assert.match(html,/Permission is required\./);assert.match(html,/>Allow<\/button>/);
  assert.match(html,/detect competing simulator instances/);assert.match(html,/Automatic game detection can still use its one-second fallback/);
  assert.match(html,/Allow may open a Windows permission prompt/);
  assert.equal(calls,0);assert.equal(JSON.stringify(instrumented.read()),JSON.stringify({busy:false,error:''}));
});

test('starting and checking show Cancel; ready shows Stop',async()=>{
  for(const state of ['starting','checking']){
    const {html}=await setup(xplane({state,message:`${state} now`}));assert.match(html,/Cancel/);
  }
  const ready=await setup(xplane({state:'ready',message:'Monitoring process.'}));assert.match(ready.html,/Monitoring process\./);assert.match(ready.html,/>Stop<\/button>/);
});

test('unsupported games are hidden and unsupported or uninstalled X-Plane cannot be allowed',async()=>{
  const other=await setup(xplane(undefined,{id:'fixture-game',provider:undefined,processMonitor:undefined,processMonitoringSupported:false}));
  assert.doesNotMatch(other.html,/Process monitoring/);
  const unsupported=await setup(xplane({state:'disabled',message:'Unsupported.'},{processMonitoringSupported:false}));assert.match(unsupported.html,/disabled/);
  const uninstalled=await setup(xplane({state:'waiting',message:'Not installed.'},{installed:false}));assert.match(uninstalled.html,/disabled/);
});

test('Assetto shows the explicitly supplied native monitoring state',async()=>{
  const {html}=await setup(xplane({state:'needs-permission',message:'Assetto process permission is required.'},
    {id:'assetto-corsa',name:'Assetto Corsa',provider:'assetto-corsa'}));
  assert.match(html,/Assetto process permission is required\./);
  assert.match(html,/>Allow<\/button>/);
});

test('explicit Allow sends only command and gameId and tracks the pending request',async()=>{
  let resolve;const sent=[];const {instrumented}=await setup(xplane(),request=>{sent.push(request);return new Promise(done=>resolve=done);});
  const pending=instrumented.request('game_process_monitor_enable');
  assert.equal(JSON.stringify(sent),JSON.stringify([{command:'game_process_monitor_enable',gameId:'x-plane-12'}]));assert.equal(instrumented.read().busy,true);
  resolve();await pending;assert.equal(instrumented.read().busy,false);assert.equal(instrumented.read().error,'');
});

test('request failures populate the accessible error state and release busy state',async()=>{
  const {instrumented}=await setup(xplane(),async()=>{throw new Error('Native request failed.');});
  await instrumented.request('game_process_monitor_enable');
  assert.equal(JSON.stringify(instrumented.read()),JSON.stringify({busy:false,error:'Native request failed.'}));
});

test('explicit Cancel sends one cancel command and never silently retries',async()=>{
  const sent=[];const {html,instrumented}=await setup(xplane({state:'checking',message:'Checking process.'}),async request=>sent.push(request));
  assert.match(html,/Cancel/);await instrumented.request('game_process_monitor_cancel');await new Promise(resolve=>setImmediate(resolve));
  assert.equal(JSON.stringify(sent),JSON.stringify([{command:'game_process_monitor_cancel',gameId:'x-plane-12'}]));assert.equal(instrumented.read().busy,false);
});
