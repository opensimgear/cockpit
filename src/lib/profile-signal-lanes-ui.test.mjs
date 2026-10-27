import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-processing']);
const processing=modules.get('./profile-processing');
const transpile=source=>ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const componentSource=await readFile(new URL('./ProfileSignalLanes.svelte',import.meta.url),'utf8');
const iconSource=await readFile(new URL('./Icon.svelte',import.meta.url),'utf8');
const componentCode=transpile(compile(componentSource,{filename:'ProfileSignalLanes.svelte',generate:'server'}).js.code);
const iconCode=transpile(compile(iconSource,{filename:'Icon.svelte',generate:'server'}).js.code);
const context={require(name){
 if(name==='svelte/internal/server')return server;
 if(name==='svelte')return svelte;
 if(name==='svelte/transition')return require('svelte/transition');
 if(name==='./Icon.svelte')return {default:Icon};
 if(name==='./profile-processing')return processing;
 throw new Error(`Unexpected dependency ${name}`);
}};
const iconExports={};vm.runInNewContext(iconCode,{...context,exports:iconExports},{filename:'Icon.svelte'});
const Icon=iconExports.default,componentExports={};vm.runInNewContext(componentCode,{...context,exports:componentExports},{filename:'ProfileSignalLanes.svelte'});
const Lanes=componentExports.default;
const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
const port=(id,direction)=>({id,name:id,direction,contract});
const stage={id:'stage',name:'Scale stage',operation:{kind:'identity'},inputs:[port('in','input')],outputs:[port('out','output')]};
const nodes=[{id:'source',name:'Source'},{id:'target',name:'Destination'},{id:'stage',name:'Scale stage',processing:stage}];
const endpoint=(nodeId,portId)=>({nodeId,portId});
const bundle=(lanes=[])=>({id:'bundle',binding:{id:'route',from:endpoint('source','out'),to:endpoint('target','in')},lanes});
const lane={id:'route',destination:endpoint('target','in'),sources:[endpoint('source','out')],transforms:['stage'],bindingIds:['route']};

function renderLanes(props){return render(Lanes,{props:{bundle:bundle(),nodes,close(){},...props}}).body;}

test('empty connection overview shows endpoints, Add and exact empty hint without editing data',()=>{
 const empty=bundle(),original=JSON.stringify(empty),body=renderLanes({bundle:empty,addConnection(){}});
 assert.match(body,/<h3 class="source-device[^\"]*">Source<\/h3>/);assert.match(body,/<h3 class="destination-device[^\"]*">Destination<\/h3>/);
 assert.match(body,/No connections yet\./);assert.match(body,/<button[^>]*class="add-connection[^\"]*"[^>]*>[\s\S]*?<\/svg><!---->Add<\/button>/);
 assert.equal(JSON.stringify(empty),original);assert.doesNotMatch(body,/No signals\./);
});

test('transform chips retain the purple chip and expose a separate destructive keyboard-accessible delete button',()=>{
 const body=renderLanes({bundle:bundle([lane]),removeTransform(){},readOnly:true});
 assert.match(body,/class="transform-chip-wrap[^\"]*"/);assert.match(body,/class="transform-chip[^\"]*"/);assert.match(body,/class="remove-transform[^\"]*"[^>]*aria-label="Delete Scale stage"[^>]*disabled/);
 assert.match(componentSource,/\.transform-chip-wrap:hover \.remove-transform,\.transform-chip-wrap:focus-within \.remove-transform/);
 assert.match(componentSource,/@media\(hover:none\)\{\.remove-transform\{opacity:1;pointer-events:auto\}\}/);assert.match(componentSource,/background:color-mix\(in srgb,#9b80d8 14%,var\(--surface\)\)/);
 assert.doesNotMatch(body,/<button[^>]*class="transform-chip"[^>]*>[\s\S]*?<button[^>]*class="remove-transform"/);
});

test('settings slide below the selected row and preserve the full-width three-column overview',()=>{
 const wrapper=`<script>import Lanes from './ProfileSignalLanes.svelte';
 const c={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
 const p=(id,direction)=>({id,name:id,direction,contract:c});
 const stage={id:'stage',name:'Scale stage',operation:{kind:'identity'},inputs:[p('in','input')],outputs:[p('out','output')]};
 const nodes=[{id:'source',name:'Source'},{id:'target',name:'Destination'},{id:'stage',name:'Scale stage',processing:stage}];
 const bundle={id:'bundle',binding:{id:'route',from:{nodeId:'source',portId:'out'},to:{nodeId:'target',portId:'in'}},lanes:[]};
 const close=()=>{};</script>
 {#snippet settings(node)}<label>Factor <input value="2"/></label>{/snippet}
 {#snippet draft()}<div>Source draft</div><div>Transform draft</div><div>Destination draft</div>{/snippet}
 <Lanes {bundle} {nodes} {close} activeTransform="stage" transformSettings={settings} draftConnection={draft}/>`;
 const wrapperCode=transpile(compile(wrapper,{filename:'SignalLanesWrapper.svelte',generate:'server'}).js.code),exports={};
 const wrappedContext={...context,require(name){if(name==='./ProfileSignalLanes.svelte')return {default:Lanes};return context.require(name);}};
 vm.runInNewContext(wrapperCode,{...wrappedContext,exports},{filename:'SignalLanesWrapper.svelte'});
 const body=render(exports.default,{}).body;
 assert.match(body,/class="signal-lane pending-connection[^\"]*"/);assert.match(body,/Source draft[\s\S]*Transform draft[\s\S]*Destination draft/);
 assert.match(body,/<div class="lane-settings[^\"]*" data-transform-settings="stage">/);
 assert.ok(body.indexOf('class="signal-lane pending-connection')<body.indexOf('class="lane-settings'),'draft settings follow the draft row');
 assert.match(componentSource,/transition:slide=\{\{duration:140\}\}/);
 assert.match(componentSource,/lanes\.find\(lane=>lane\.id===selectedLaneId&&lane\.transforms\.includes\(activeTransform\)\)\?\?lanes\.find\(lane=>lane\.transforms\.includes\(activeTransform\)\)/,'programmatic selections resolve to a row, while a clicked row takes priority');
 assert.match(componentSource,/selectTransform\(laneId,id\)/);
 assert.doesNotMatch(componentSource,/<aside class="lane-settings/);
 assert.doesNotMatch(componentSource,/overview-content\.has-settings/);
 assert.match(componentSource,/@render removeButton\(node,lane\.id\)/,'row delete callback receives its lane id');
 assert.doesNotMatch(componentSource,/settings-close|Close transformation settings/);
});

test('programmatically active shared transform opens beneath the first matching row',()=>{
 const secondLane={...lane,id:'route-2'};
 const body=render(Lanes,{props:{bundle:bundle([lane,secondLane]),nodes,activeTransform:'stage',close(){},transformSettings(){return 'Factor';}}}).body;
 assert.equal((body.match(/data-transform-settings="stage"/g)??[]).length,1);
 const firstRow=body.indexOf('data-lane-id="route"'),firstSettings=body.indexOf('data-transform-settings="stage"'),secondRow=body.indexOf('data-lane-id="route-2"');
 assert.ok(firstRow<firstSettings&&firstSettings<secondRow,'settings follow the first row that contains the active transform');
});

test('standalone transform library keeps its settings available below the library',()=>{
 const body=render(Lanes,{props:{nodes,bundle:undefined,activeTransform:'stage',close(){},transformSettings(node){return node.name;}}}).body;
 assert.match(body,/Transform settings/);
 assert.match(body,/<div class="lane-settings[^\"]*" data-transform-settings="stage">/);
 assert.ok(body.indexOf('class="transform-library')<body.indexOf('class="lane-settings'),'library settings follow the transform library');
});

test('incomplete connection stays in the original row with its error and Add footer',()=>{
 const row={...lane,transforms:[]};const body=render(Lanes,{props:{bundle:bundle([row]),nodes,close(){},activeIncompleteRowId:row.id,draftConnection(){},routeIssue(){return 'Choose a matching transform.';},addConnection(){}}}).body;
 assert.match(body,/data-lane-id="route"/);assert.match(body,/aria-label="Incomplete connection"/);assert.match(body,/Connection error: Choose a matching transform/);assert.match(body,/>Add<\/button>/);assert.doesNotMatch(body,/New connection draft|pending-connection/);
});
