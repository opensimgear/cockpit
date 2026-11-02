import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules} from './test-module-loader.mjs';

/** Compile the real process monitor and icon for isolated server-rendered UI tests. */
export async function loadProcessMonitor(modules, gatewayRequest, optionalinstrument=false) {
  const componentUrl=new URL('./GameProcessMonitor.svelte',import.meta.url);
  const iconUrl=new URL('./Icon.svelte',import.meta.url);
  const iconExports={};
  const iconBuilt=compile(await readFile(iconUrl,'utf8'),{filename:iconUrl.pathname,generate:'server'});
  vm.runInNewContext(ts.transpileModule(iconBuilt.js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports:iconExports,TextEncoder,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;throw Error(`Unexpected icon dependency ${name}`);}},{filename:iconUrl.pathname});
  let source=await readFile(componentUrl,'utf8');
  if(optionalinstrument)source=source.replace('</script>',"captureProcessMonitor({request,read:()=>({busy,error})});</script>");
  const built=compile(source,{filename:componentUrl.pathname,generate:'server'}),exports={};
  let instrumented;
  vm.runInNewContext(ts.transpileModule(built.js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,Error,captureProcessMonitor:value=>instrumented=value,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name==='./Icon.svelte')return iconExports;if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest};throw Error(`Unexpected process monitor dependency ${name}`);}},{filename:componentUrl.pathname});
  return {exports,get instrumented(){return instrumented;}};
}

export async function processMonitorModules(){return loadModules(['./games']);}
