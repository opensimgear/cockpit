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

test('pending draft row is three-column content and settings occupy a left pane beside the overview',()=>{
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
 assert.match(body,/<aside class="lane-settings[^\"]*" data-transform-settings="stage">/);
 assert.ok(body.indexOf('class="lane-settings svelte-')<body.indexOf('class="lane-scroll svelte-'),'settings pane is the left sibling of overview content');
 assert.match(componentSource,/\.overview-content\.has-settings\{grid-template-columns:minmax\(220px,\.65fr\) minmax\(0,1\.6fr\)/);
 assert.match(componentSource,/@media\(max-width:850px\)\{\.overview-content\.has-settings\{grid-template-columns:minmax\(0,1fr\)/);
});
