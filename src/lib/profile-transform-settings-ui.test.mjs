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
    if(name==='./ProfileTransformSettings.svelte')source=source.replace('</script>','captureSettings({parameter,type});</script>');
    if(name==='./ProfileProcessingEditor.svelte')source=source.replace('</script>','captureProcessing({field,ruleField,save,get draft(){return draft;},get portsEditable(){return portsEditable;}});</script>');
    const {js}=compile(source,{filename:filename.pathname,generate:'server'}),dependencies=new Map();
    for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(match[1],{default:await component(match[1])});
    const exports={};vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,captureSettings:value=>settings=value,captureProcessing:value=>processing=value,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(dependencies.has(name))return dependencies.get(name);const key=name.replace(/\.ts$/,'');if(modules.has(key))return modules.get(key);throw Error(`Unexpected transformation settings dependency ${name}`);}},{filename:filename.pathname});
    components.set(name,exports.default);return exports.default;
  }
  const Component=await component('./ProfileTransformSettings.svelte'),body=render(Component,{props:{node,change:()=>true,configure:()=>true,...props}}).body;
  return {body,settings,processing};
}

test('pending transformations expose saved type and parameters while their declared ports are disabled',async()=>{
  const node=create('script','pending-script'),original=plain(node),h=await view(node);
  assert.ok(h.body.includes('Transformation type'));assert.ok(h.body.includes('Needs input.'));assert.ok(h.body.includes('Connect the first input before editing declared ports.'));
  assert.match(h.body.match(/Declared ports[^]*?(<fieldset\b[^>]*>)/)?.[1]??'',/\sdisabled(?:=|\s|>)/);
  assert.equal(h.processing.portsEditable,false);assert.deepEqual(plain(node),original);
  assert.equal((h.body.match(/Typed Rhai source/g)??[]).length,1);
  assert.doesNotMatch(h.body,/>Source<input/,'Script source belongs in its multiline advanced editor.');
});

test('resolved transformations keep advanced declared-port editing enabled without the pending type selector',async()=>{
  const node=processingTemplate('script','resolved-script',state()),h=await view(node);
  assert.equal(h.processing.portsEditable,true);assert.ok(!h.body.includes('Transformation type'));assert.ok(!h.body.includes('Needs input.'));
  const declared=h.body.slice(h.body.indexOf('Declared ports'));assert.doesNotMatch(declared.match(/<fieldset\b[^>]*>/)?.[0]??'',/\sdisabled(?:=|\s|>)/);
  assert.ok(declared.includes('Add input port'));assert.ok(declared.includes('Add output port'));
});

test('rejected numeric changes restore the displayed saved value and accepted changes retain the entered number',async()=>{
  const node=processingTemplate('scale','scale',state()),before=plain(node),edits=[];
  let h=await view(node,{change:next=>{edits.push(plain(next));return false;}}),input={value:'20'};
  h.settings.parameter('factor',input);assert.equal(input.value,'1');assert.equal(edits[0].operation.factor,20);assert.deepEqual(plain(node),before);
  h=await view(node,{change:next=>{edits.push(plain(next));return true;}});input={value:'0.5'};h.settings.parameter('factor',input);assert.equal(input.value,'0.5');assert.equal(edits.at(-1).operation.factor,.5);
});

test('rejected pending parameters and type choices restore their saved controls without changing the block',async()=>{
  const node=create('scale','pending',{factor:10}),before=plain(node),calls=[];
  let h=await view(node,{configure:(preset,parameters)=>{calls.push([preset,plain(parameters??{})]);return false;}}),input={value:'25'};
  h.settings.parameter('factor',input);assert.equal(input.value,'10');assert.deepEqual(calls[0],['scale',{factor:25}]);
  const select={value:'clamp'};h.settings.type(select);assert.equal(select.value,'scale');assert.deepEqual(calls[1],['clamp',{}]);assert.deepEqual(plain(node),before);
  h=await view(node,{configure:()=>true});select.value='clamp';h.settings.type(select);assert.equal(select.value,'clamp');
});

test('rejected string parameters restore the exact declared value rather than retaining a failed edit',async()=>{
  const node=processingTemplate('format','text',state()),edits=[],h=await view(node,{change:next=>{edits.push(plain(next));return false;}}),input={value:'invalid suffix'};
  h.settings.parameter('suffix',input);assert.equal(input.value,'');assert.equal(edits[0].operation.suffix,'invalid suffix');assert.equal(node.operation.suffix,'');
});

test('advanced pending timing parameters persist through first-input resolution with their operation fields marked configured',async()=>{
  let value=drop(graph(),'pulse');const original=plain(value);
  const h=await view(value.document.processingNodes[0],{change:node=>{value=update(value,node);return true;}});
  h.processing.ruleField('durationUs',125000);h.processing.ruleField('repeatUs',500000);h.processing.save();
  assert.deepEqual(plain(value.document.processingNodes[0].draft.configured),['rule']);
  assert.equal(value.document.processingNodes[0].operation.rule.durationUs,125000);assert.equal(value.document.processingNodes[0].operation.rule.repeatUs,500000);
  const resolved=resolve(value,'transform-1',state('boolean')).document.processingNodes[0];
  assert.equal(resolved.draft,undefined);assert.equal(resolved.operation.rule.durationUs,125000);assert.equal(resolved.operation.rule.repeatUs,500000);assert.deepEqual(plain(resolved.inputs),[{...plain(resolved.inputs[0]),contract:state('boolean')}]);
  assert.equal(original.document.processingNodes[0].operation.rule.durationUs,100000);
});

test('advanced pending script source survives first-input resolution while its declared ports remain protected',async()=>{
  let value=drop(graph(),'script');const source='fn on_event(ctx) { transaction(ctx.state, #{out:scalar(42)}, []) }';
  const h=await view(value.document.processingNodes[0],{change:node=>{value=update(value,node);return true;}});assert.equal(h.processing.portsEditable,false);
  h.processing.field('source',source);h.processing.save();assert.deepEqual(plain(value.document.processingNodes[0].draft.configured),['source']);
  const resolved=resolve(value,'transform-1',state()).document.processingNodes[0];assert.equal(resolved.operation.source,source);assert.equal(resolved.inputs[0].contract.schema.unit,'rpm');
});
