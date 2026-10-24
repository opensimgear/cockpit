import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';

async function component(name){
  const file=new URL(name,import.meta.url),{js}=compile(await readFile(file,'utf8'),{filename:file.pathname,generate:'server'}),dependencies=new Map(),exports={};
  for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(match[1],{default:await component(match[1])});
  vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(dependencies.has(name))return dependencies.get(name);throw Error(name);}});
  return exports.default;
}
const PageHeader=await component('./PageHeader.svelte');
const html=props=>render(PageHeader,{props}).body.replace(/<!--[\s\S]*?-->/g,'');
const snippet=markup=>svelte.createRawSnippet(()=>({render:()=>markup}));

test('root pages have a semantic title and subtitle without a spurious Back button',()=>{
  const body=html({title:'Games',subtitle:'Your installed games',eyebrow:'Your games'});
  assert.match(body,/<h1\b[^>]*>Games<\/h1>/);assert.match(body,/Your installed games/);assert.match(body,/Your games/);assert.doesNotMatch(body,/<button/);
});
test('detail pages keep a disabled, labeled Back action with its icon',()=>{
  const body=html({title:'Toggle',onback:()=>{throw Error('Rendering must not navigate');},backLabel:'Back to Widgets',backDisabled:true});
  assert.match(body,/<button\b[^>]*disabled[^>]*>/);assert.match(body,/<svg\b/);assert.match(body,/Back to Widgets/);
});
test('editable headings retain their accessible name and expose page actions',()=>{
  const body=html({title:'My profile',titleContent:snippet('<input aria-label="Profile name" value="My profile"/>'),actions:snippet('<button>Save</button>'),leading:snippet('<span>Connected</span>')});
  assert.match(body,/<h1\b[^>]*aria-label="My profile"[^>]*><input/);assert.match(body,/<button>Save<\/button>/);assert.match(body,/Connected/);
});
test('section titles use h2 and omit absent optional copy',()=>{
  const body=html({title:'Widgets',level:2});assert.match(body,/<h2\b[^>]*>Widgets<\/h2>/);assert.doesNotMatch(body,/<h1|<p\b|header-eyebrow/);
});
