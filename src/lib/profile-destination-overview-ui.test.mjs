import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules,libModuleName} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-canvas/device-identity','./canvas-navigation','./canvas-styling','./profile-canvas','./profile-canvas-render',
  './profile-diagram','./profile-editor','./profile-input-availability','./profile-inventory',
  './profile-overview-layout','./profile-overview-routing','./profile-port-browser',
  './profile-palette','./profile-processing','./profile-signal-overview','./profile-vehicle-tabs','./profile-wire-layout',
]);
const plain=value=>JSON.parse(JSON.stringify(value));
const port=(id,direction)=>({id,name:id,direction,contract:{delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null}});
const resource=(id,name,ports)=>({id,name,kind:'device',required:false,ports});
const document=()=>({
  schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Destination overview',revision:1,
  eligibility:{gameIds:[]},
  resources:[
    resource('stick','Joystick',[port('roll','output'),port('direct','output'),port('other','output')]),
    resource('pedals','Pedals',[port('rudder','output')]),
    resource('flight','Flight controls',[port('axis','input'),port('button','input')]),
    resource('keyboard','Keyboard',[port('key','input')]),
  ],bindings:[
    {id:'stick-to-mix',from:{nodeId:'stick',portId:'roll'},to:{nodeId:'mix',portId:'a'}},
    {id:'pedals-to-mix',from:{nodeId:'pedals',portId:'rudder'},to:{nodeId:'mix',portId:'b'}},
    {id:'multi-input-route',from:{nodeId:'mix',portId:'out'},to:{nodeId:'flight',portId:'axis'}},
    {id:'direct-route',from:{nodeId:'stick',portId:'direct'},to:{nodeId:'flight',portId:'button'}},
    {id:'other-destination',from:{nodeId:'stick',portId:'other'},to:{nodeId:'keyboard',portId:'key'}},
  ],processingNodes:[{id:'mix',name:'Mixer',operation:{kind:'identity'},inputs:[],outputs:[]}],
  virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],
  editor:{positions:[],viewport:{x:0,y:0,zoom:1}},
});

async function components(hooks={}){
  const cache=new Map();
  async function component(name){
    if(cache.has(name))return cache.get(name);
    const filename=new URL(name,import.meta.url);let source=await readFile(filename,'utf8');
    if(name==='./ProfileCanvas.svelte'){
      source=source.replace('</script>',`overviewPair=seedOverviewPair();overviewBundleId=seedOverviewBundleId();captureCanvas({openOverview,focusConnectionTransform,get state(){return{overviewPair,overviewBundleId,overviewBundle,overviewBundles,overviewById,destinationBundles,destinationById,activeTransform};}});</script>`);
      assert.notEqual(source,await readFile(filename,'utf8'),'Canvas instrumentation must match the production script.');
    }
    const {js}=compile(source,{filename:filename.pathname,generate:'server'}),dependencies=new Map();
    for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(libModuleName(match[1],filename),{default:await component(libModuleName(match[1],filename))});
    const exports={};
    vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{
      exports,TextEncoder,seedOverviewPair:()=>hooks.overviewPair??null,seedOverviewBundleId:()=>hooks.overviewBundleId??'',captureCanvas:value=>hooks.captureCanvas?.(value),
      require(specifier){
        const dependency=libModuleName(specifier,filename);
        if(dependency==='svelte/internal/server')return server;
        if(dependency==='svelte')return svelte;
        if(dependencies.has(dependency))return dependencies.get(dependency);
        const key=dependency.replace(/\.ts$/,'');
        if(modules.has(key))return modules.get(key);
        throw new Error(`Unexpected UI dependency ${specifier}`);
      },
    },{filename:filename.pathname});
    cache.set(name,exports.default);return exports.default;
  }
  return component;
}

async function canvasHarness(value,hooks={}){
  let handlers;const component=await components({...hooks,captureCanvas:state=>handlers=state});
  const Canvas=await component('./ProfileCanvas.svelte'),edits=[];
  const body=render(Canvas,{props:{document:value,assignments:[],inventory:[],inventoryClient:{inspect(){throw Error('SSR must not inspect inventory.');}},selectedId:'',readOnly:true,change:(next,label)=>edits.push({next,label})}}).body;
  return{body,handlers,edits};
}

test('canvas destination panel aggregates source rows, deduplicates shared routes, and switches arrows by destination',async()=>{
  const original=document(),before=plain(original);
  const projected=modules.get('./profile-signal-overview').destinationOverview(modules.get('./profile-signal-overview').signalOverview(original));
  const destination=projected.find(bundle=>bundle.binding.to.nodeId==='flight');
  const h=await canvasHarness(original,{overviewPair:{from:'stick',to:'flight'},overviewBundleId:destination.id});
  const sourceTitle=h.body.match(/<h3 class="source-device[^\"]*">([^<]+)<\/h3>/)?.[1]??'';
  assert.ok(sourceTitle.includes('Joystick')&&sourceTitle.includes('Pedals'),sourceTitle);
  assert.ok(!sourceTitle.includes('Keyboard'),sourceTitle);
  assert.ok(h.body.includes('Flight controls'));
  assert.match(h.body,/data-lane-id="multi-input-route"/);
  assert.match(h.body,/data-lane-id="direct-route"/);
  assert.doesNotMatch(h.body,/data-lane-id="other-destination"/);
  assert.equal((h.body.match(/data-lane-id="multi-input-route"/g)??[]).length,1);
  assert.deepEqual(plain(h.handlers.state.overviewBundle.lanes.map(lane=>lane.id)),['multi-input-route','direct-route']);
  assert.ok(h.handlers.state.overviewById.has('overview:["stick","flight"]'));
  assert.ok(h.handlers.state.destinationById.has(destination.id));

  h.handlers.openOverview('overview:["stick","flight"]');
  const first=h.handlers.state;
  assert.deepEqual(plain(first.overviewPair),{from:'stick',to:'flight'});
  assert.equal(first.overviewBundleId,destination.id);
  h.handlers.openOverview('overview:["pedals","flight"]');
  assert.deepEqual(plain(h.handlers.state.overviewPair),{from:'pedals',to:'flight'});
  assert.equal(h.handlers.state.overviewBundleId,destination.id,'Arrows to one destination select the same panel.');
  assert.deepEqual(plain(h.handlers.state.overviewBundle.lanes.map(lane=>lane.id)),['multi-input-route','direct-route']);
  assert.equal(h.handlers.state.overviewBundle.lanes.filter(lane=>lane.id==='multi-input-route').length,1);
  assert.deepEqual(h.edits,[]);
  assert.deepEqual(original,before,'Read-only rendering and arrow selection must not mutate the profile draft.');
});

test('processing focus opens its owning destination overview and orphan transforms have no standalone route',async()=>{
  const original=document(),hooks={overviewPair:{from:'stick',to:'flight'}};
  original.bindings=original.bindings.filter(binding=>binding.id==='stick-to-mix'||binding.id==='multi-input-route');
  const processing=modules.get('./profile-processing');
  original.processingNodes=[processing.processingTemplate('gate','mix',port('fixture','output').contract)];
  original.processingNodes[0].inputs[0].name='Enable';
  original.processingNodes[0].inputs[1].name='Signal';
  original.processingNodes[0].outputs[0].name='Result';
  const h=await canvasHarness(original,hooks),destination=modules.get('./profile-signal-overview').destinationOverview(modules.get('./profile-signal-overview').signalOverview(original)).find(bundle=>bundle.binding.to.nodeId==='flight');
  h.handlers.focusConnectionTransform('mix');
  assert.equal(h.handlers.state.overviewBundleId,destination.id);
  assert.deepEqual(plain(h.handlers.state.overviewPair),{from:'stick',to:'flight'});
  assert.equal(h.handlers.state.activeTransform,'mix');

  const orphan=processing.processingTemplate('gate','orphan',port('fixture','output').contract);
  original.processingNodes=[orphan];
  const standalone=await canvasHarness(original);
  standalone.handlers.focusConnectionTransform('orphan');
  assert.equal(standalone.handlers.state.overviewBundleId,'');
  assert.equal(standalone.handlers.state.activeTransform,'');
});
