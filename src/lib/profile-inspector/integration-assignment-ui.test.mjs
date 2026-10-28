import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules,libModuleName} from '../test-module-loader.mjs';
const modules=await loadModules(['./profile-canvas','./profile-graph']);
const transpile=source=>ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const cache=new Map();
async function component(name,capture,parent=import.meta.url){
 const filename=new URL(name,parent),key=libModuleName(name,parent);
 if(!capture&&cache.has(key))return cache.get(key);
 let source=await readFile(filename,'utf8');
 if(capture)source=source.replace('</script>','captureAssignment({choose});</script>');
 const {js}=compile(source,{filename:filename.pathname,generate:'server'}),dependencies=new Map();
 for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)){
  const dependency=libModuleName(match[1],filename);dependencies.set(dependency,{default:await component(match[1],undefined,filename)});
 }
 const exports={};vm.runInNewContext(transpile(js.code),{exports,TextEncoder,captureAssignment:capture,require(specifier){
  const dependency=libModuleName(specifier,filename);
  if(dependency==='svelte/internal/server')return server;
  if(dependency==='svelte')return svelte;
  if(dependencies.has(dependency))return dependencies.get(dependency);
  const key=dependency.replace(/\.ts$/,'');if(modules.has(key))return modules.get(key);
  throw Error(`Unexpected component dependency ${specifier}`);
 }},{filename:filename.pathname});
 if(!capture)cache.set(key,exports.default);return exports.default;
}
const state={delivery:'state',schema:{type:'integer',min:0,max:65535,unit:null},freshnessMs:1500,fallback:null};
const action={delivery:'action',parameters:[{id:'value',name:'Value',schema:{type:'integer',min:0,max:65535,unit:null}}],semantics:'setter',endpoint:'LIGHT',readable:false,display:null,ownership:null};
const hash=`sha256:${'a'.repeat(64)}`;
const observed=(id,direction,contract,current=true)=>({descriptor:{id,name:`Native ${id}`,direction,contract},contractHash:hash,current,routeIds:['native']});
const options=[observed('LIGHT','output',state),observed('SET_LIGHT','input',action),observed('STALE','output',state,false),observed('WRONG','output',{...state,freshnessMs:300})];
const ports=[{id:'telemetry',name:'Saved brightness',direction:'output',contract:state},{id:'setter',name:'Saved lighting command',direction:'input',contract:action}];
const key=id=>JSON.stringify([id,hash]);
async function harness({busy=false,readOnly=false,run=()=>{}}={}){
 const submitted=[];const Component=await component('./ProfileIntegrationAssignment.svelte',handlers=>run(handlers,submitted));
 const body=render(Component,{props:{ports,options,busy,readOnly,assign:selections=>{submitted.push(selections);return true;}}}).body;
 return {body,submitted};
}

test('renders explicit matching selectors without guessing or selecting saved signals',async()=>{
 const {body,submitted}=await harness();assert.match(body,/Match saved signals/);assert.match(body,/Saved brightness/);assert.match(body,/Saved lighting command/);assert.deepEqual(submitted,[]);
 assert.match(body,/Choose a matching signal/);
});

test('submits only after all explicit choices and retains exact logical and native identities',async()=>{
 const {submitted}=await harness({run({choose},calls){choose('telemetry',key('LIGHT'));assert.equal(calls.length,0);choose('setter',key('SET_LIGHT'));}});
 assert.equal(submitted.length,1);assert.deepEqual(JSON.parse(JSON.stringify(submitted[0].map(item=>[item.profilePortId,item.observed.descriptor.id]))),[['telemetry','LIGHT'],['setter','SET_LIGHT']]);
});

test('busy and read-only handlers never submit selections',async()=>{
 for(const guard of [{busy:true},{readOnly:true}]){
  const {body,submitted}=await harness({...guard,run({choose}){choose('telemetry',key('LIGHT'));choose('setter',key('SET_LIGHT'));}});
  assert.equal(submitted.length,0);assert.match(body,/disabled/);
 }
});

test('incompatible, stale, unknown and wrong-direction choices never complete assignment',async()=>{
 for(const invalid of ['STALE','WRONG','SET_LIGHT','missing']){
  const {submitted}=await harness({run({choose}){choose('missing',key('LIGHT'));choose('telemetry',key(invalid));choose('setter',key('SET_LIGHT'));}});assert.equal(submitted.length,0);
 }
});
