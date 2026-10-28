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
const modules=await loadModules(['./profile-transform-drafts','./profile-editor','./profile-canvas']);
const {createProcessingDraft:create,graphDropProcessing:drop,graphUpdateProcessing:update,resolveProcessingDraft:resolve}=modules.get('./profile-transform-drafts');
const {blankProfile}=modules.get('./profile-editor');
const {processingTemplate}=modules.get('./profile-processing');
const plain=value=>JSON.parse(JSON.stringify(value));
const state=(type='number')=>({delivery:'state',schema:type==='boolean'?{type}:{type,min:0,max:100,unit:'rpm'},freshnessMs:350,fallback:null});
const graph=()=>({document:blankProfile('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'),assignments:[]});

async function view(node,props={}){
  const components=new Map();let settings,processing;
  async function component(name){
    if(components.has(name))return components.get(name);
    const filename=new URL(name,import.meta.url);let source=await readFile(filename,'utf8');
    if(name==='./ProfileTransformSettings.svelte')source=source.replace('</script>','captureSettings({type});</script>');
    if(name==='./ProfileProcessingEditor.svelte')source=source.replace('</script>','captureProcessing({field,ruleField,save,autoSave,get draft(){return draft;},get portsEditable(){return portsEditable;},get compact(){return compact;}});</script>');
    const {js}=compile(source,{filename:filename.pathname,generate:'server'}),dependencies=new Map();
    for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(libModuleName(match[1],filename),{default:await component(libModuleName(match[1],filename))});
    const exports={};vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,captureSettings:value=>settings=value,captureProcessing:value=>processing=value,require(name){name=libModuleName(name,filename);if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(dependencies.has(name))return dependencies.get(name);const key=name.replace(/\.ts$/,'');if(modules.has(key))return modules.get(key);throw Error(`Unexpected transformation settings dependency ${name}`);}},{filename:filename.pathname});
    components.set(name,exports.default);return exports.default;
  }
  const Component=await component('./ProfileTransformSettings.svelte'),body=render(Component,{props:{node,change:()=>true,configure:()=>true,...props}}).body;
  return {body,settings,processing};
}


test('identity and Boolean NOT expose only their type, without settings or metadata',async()=>{
 for(const kind of ['identity','not']){
  const node=processingTemplate(kind,kind,state('boolean')),before=plain(node),h=await view(node);
  assert.equal((h.body.match(/<select\b/g)??[]).length,1);
  assert.doesNotMatch(h.body,/<(?:input|textarea|button|details|h4|p)\b/);
  assert.doesNotMatch(h.body,/Block label|Declared ports|Advanced|Update visible node/);
  assert.equal(h.processing.compact,true);assert.deepEqual(plain(node),before);
 }
});

test('scale exposes its two settings once alongside the type selector',async()=>{
 const h=await view(processingTemplate('scale','scale',state()));
 assert.match(h.body,/Multiply by/);assert.match(h.body,/Offset/);
 assert.equal((h.body.match(/type="number"/g)??[]).length,2);
 assert.doesNotMatch(h.body,/Visible transformation|Declared ports|Block label|Advanced|Update visible node/);
});

test('valid scalar edits derive the output contract and save directly',async()=>{
 const node=processingTemplate('scale','scale',state()),before=plain(node),edits=[],h=await view(node,{change:next=>{edits.push(plain(next));return true;}});
 h.processing.field('factor',.5);assert.equal(h.processing.autoSave(),true);
 assert.equal(edits[0].operation.factor,.5);assert.equal(edits[0].outputs[0].contract.schema.max,50);
 assert.deepEqual(plain(node),before);
});

test('invalid settings stay editable and do not change the saved transform',async()=>{
 const node=processingTemplate('clamp','clamp',state()),edits=[],h=await view(node,{change:next=>{edits.push(plain(next));return true;}});
 h.processing.field('min',80);h.processing.field('max',20);assert.equal(h.processing.autoSave(),false);assert.equal(edits.length,0);
 assert.equal(h.processing.draft.operation.min,80);
 h.processing.field('min',10);assert.equal(h.processing.autoSave(),true);assert.equal(edits[0].operation.min,10);assert.equal(edits[0].operation.max,20);
});

test('a rejected parameter change cannot mutate the supplied saved node',async()=>{
 const node=processingTemplate('compare','compare',state()),before=plain(node),h=await view(node,{change:()=>false});
 h.processing.field('threshold',70);assert.equal(h.processing.autoSave(),false);assert.deepEqual(plain(node),before);
});

test('pending and resolved type choices use the same compact selector and revert rejected choices',async()=>{
 const node=create('scale','pending'),before=plain(node),calls=[],h=await view(node,{configure:kind=>{calls.push(kind);return false;}}),input={value:'clamp'};
 h.settings.type(input);assert.equal(input.value,'scale');assert.deepEqual(calls,['clamp']);assert.deepEqual(plain(node),before);
 const resolved=await view(processingTemplate('not','not',state('boolean')),{typeOptions:[{value:'not',label:'Boolean NOT'},{value:'identity',label:'Identity'}],changeType:kind=>{calls.push(kind);return true;}});
 input.value='identity';resolved.settings.type(input);assert.equal(input.value,'identity');assert.equal(calls.at(-1),'identity');
});

test('builtin timing fields remain compact and persist through first-input resolution',async()=>{
 let value=drop(graph(),'pulse');const h=await view(value.document.processingNodes[0],{change:node=>{value=update(value,node);return true;}});
 assert.match(h.body,/Duration Us/);assert.match(h.body,/Repeat Us/);assert.doesNotMatch(h.body,/Declared ports|Update visible node/);
 h.processing.ruleField('durationUs',125000);h.processing.ruleField('repeatUs',500000);assert.equal(h.processing.autoSave(),true);
 assert.deepEqual(plain(value.document.processingNodes[0].draft.configured),['rule']);
 const resolved=resolve(value,'transform-1',state('boolean')).document.processingNodes[0];
 assert.equal(resolved.operation.rule.durationUs,125000);assert.equal(resolved.operation.rule.repeatUs,500000);
});

test('script source remains editable without exposing port declarations or descriptions',async()=>{
 let value=drop(graph(),'script');const source='fn on_event(ctx) { transaction(ctx.state, #{out:scalar(42)}, []) }';
 const h=await view(value.document.processingNodes[0],{change:node=>{value=update(value,node);return true;}});
 assert.equal(h.processing.portsEditable,false);assert.equal((h.body.match(/<textarea\b/g)??[]).length,1);
 assert.doesNotMatch(h.body,/Declared ports|Needs input|Typed script parameters|ctx.inputs/);
 h.processing.field('source',source);assert.equal(h.processing.autoSave(),true);
 const resolved=resolve(value,'transform-1',state()).document.processingNodes[0];assert.equal(resolved.operation.source,source);assert.equal(resolved.inputs[0].contract.schema.unit,'rpm');
});

test('deadzone settings retain bounded percentages without instructional text',async()=>{
 const node=create('deadzone','pending-deadzone',{centerPercent:50,deadzonePercent:10}),h=await view(node);
 assert.match(h.body,/Center \(%\)/);assert.match(h.body,/Deadzone/);
 assert.match(h.body,/min="0" max="100"/);assert.doesNotMatch(h.body,/45–55|neutral band|Advanced/);
});


test('the delete action sits below type in its separated column',async()=>{
 const h=await view(processingTemplate('identity','identity',state('boolean')),{remove(){}});
 assert.match(h.body,/class="transform-type-controls/);
 assert.match(h.body,/class="delete-transform[^\"]*" aria-label="Delete Identity"/);
 assert.ok(h.body.indexOf('Transformation type')<h.body.indexOf('class="delete-transform'));
 assert.equal((h.body.match(/<button\b/g)??[]).length,1);assert.equal((h.body.match(/<input\b/g)??[]).length,0);
});
