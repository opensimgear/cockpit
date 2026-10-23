import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import * as compiler from '../../packages/widget-compiler/src/index.ts';
import * as artifact from '../../packages/widget-runtime/src/artifact.ts';
import * as behavior from '../../packages/widget-runtime/src/behavior.ts';
import * as editor from './generic-widget-editor.ts';

const filename=new URL('./GenericWidgetDesigner.svelte',import.meta.url);
const source=await readFile(filename,'utf8');
const flush=async()=>{for(let i=0;i<10;i++)await Promise.resolve();};
function harness(initial=''){
  let api;const builds=[],exports={};
  const input=source.replace('</script>',`${initial}\ncapture({openWorkspace,invalidate,compile,get busy(){return busy},get compiled(){return compiled},get progress(){return compilationProgress},get error(){return error}});</script>`);
  const js=compile(input,{filename:filename.pathname,generate:'server'}).js.code;
  vm.runInNewContext(ts.transpileModule(js,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{
    exports,structuredClone,crypto,console,capture:value=>api=value,
    require(name){
      if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;
      if(name.endsWith('.svelte'))return {default(){}};
      if(name==='../../packages/widget-compiler/src/index.ts')return {...compiler,compileWidget:(snapshot,options)=>new Promise((resolve,reject)=>builds.push({snapshot,options,resolve,reject}))};
      if(name==='../../packages/widget-runtime/src/artifact.ts')return artifact;
      if(name==='../../packages/widget-runtime/src/behavior.ts')return behavior;
      if(name==='./generic-widget-editor.ts')return editor;
      throw Error(name);
    },
  });
  const html=render(exports.default,{props:{source:compiler.createToggleSource(),onsave:async()=>{},oncancel(){}}}).body;
  return {api,builds,html};
}
test('Preview replaces Try and opening it compiles once, tracks progress and reuses a current build',async()=>{
  const h=harness();assert.match(h.html,/>Preview<\/button>/);assert.doesNotMatch(h.html,/>Try<\/button>/);
  h.api.openWorkspace('Preview');assert.equal(h.builds.length,1);assert.equal(h.api.busy,true);
  assert.equal(h.api.progress,undefined);
  h.builds[0].options.onProgress({phase:'rasterizing',completed:2,total:8});assert.equal(h.api.progress,.25);
  h.api.openWorkspace('Design');h.api.openWorkspace('Preview');assert.equal(h.builds.length,1);
  const bundle={artifact:{width:240,height:144}};h.builds[0].resolve(bundle);await flush();
  assert.equal(h.api.compiled,bundle);assert.equal(h.api.busy,false);assert.equal(h.api.progress,1);
  h.api.openWorkspace('Design');h.api.openWorkspace('Preview');assert.equal(h.builds.length,1);
});
test('cancel discards late progress and results; reopening Preview builds afresh',async()=>{
  const h=harness();h.api.openWorkspace('Preview');const old=h.builds[0];h.api.invalidate();
  assert.equal(old.options.signal.aborted,true);assert.equal(h.api.busy,false);
  old.options.onProgress({phase:'rasterizing',completed:9,total:10});old.resolve({stale:true});await flush();
  assert.equal(h.api.progress,undefined);assert.equal(h.api.compiled,undefined);
  h.api.openWorkspace('Design');h.api.openWorkspace('Preview');assert.equal(h.builds.length,2);assert.equal(h.api.busy,true);
  h.builds[1].resolve({fresh:true});await flush();assert.deepEqual(h.api.compiled,{fresh:true});
});
test('compile failure stops loading and permits retry without an automatic retry loop',async()=>{
  const h=harness();h.api.openWorkspace('Preview');h.builds[0].reject(Error('Invalid design'));await flush();
  assert.equal(h.api.busy,false);assert.match(h.api.error,/Invalid design/);assert.equal(h.builds.length,1);
  const retry=h.api.compile();assert.equal(h.builds.length,2);assert.equal(h.api.error,'');
  h.builds[1].resolve({valid:true});await retry;assert.deepEqual(h.api.compiled,{valid:true});
});
test('loading renders an accessible progress bar and cancel action in the preview workspace',()=>{
  const h=harness("tab='Preview';busy=true;compilationProgress=.25;");
  assert.match(h.html,/class="preview-loading[^\"]*"[^>]*aria-label="Preparing preview"[^>]*aria-busy="true"/);
  assert.match(h.html,/<progress[^>]*aria-label="Widget compilation"[^>]*value="0.25"/);
  assert.match(h.html,/25%/);assert.match(h.html,/>Cancel<\/button>/);assert.doesNotMatch(h.html,/Compile this design to try it/);
});
