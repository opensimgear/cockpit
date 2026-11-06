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
const modules=await loadModules(['./ui-settings-guard','./profiles','./profile-desktop-blocks','./profile-desktop-runtime']);
const {desktopBlock}=modules.get('./profile-desktop-blocks'),plain=value=>JSON.parse(JSON.stringify(value));
async function component(name,send,selection=''){
 const filename=new URL(name,import.meta.url);let source=await readFile(filename,'utf8'),handlers;
 source=source.replace("selectedId=$state('')",`selectedId=$state(${JSON.stringify(selection)})`);
 source=source.replace('</script>','captureDesktopUI({update,field,mode,scope,get state(){return{selectedId,error}}});</script>');
 const {js}=compile(source,{filename:filename.pathname,generate:'server'}),exports={};
 vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,captureDesktopUI:value=>handlers=value,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name==='./Icon.svelte')return{default:()=>{}};if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:send};throw Error(`Unexpected dependency ${name}`);}},{filename:filename.pathname});
 return{render(props={}){return{html:render(exports.default,{props}).body,get handlers(){return handlers;}};}};
}
const never=()=>{throw Error('Editing must not submit native input.');};

test('complete keyboard and mouse editors expose compact input grids and immediate draft controls',async()=>{
 for(const kind of ['keyboard','mouse']){
  const output=desktopBlock(kind,kind),changes=[],view=await component('./ProfileDesktopEditor.svelte',never);
  const {html,handlers:h}=view.render({output,change:value=>changes.push(plain(value))});
  assert.match(html,new RegExp(`aria-label="${output.name} editor"`));assert.match(html,/>Label</);assert.match(html,/> Required</);assert.match(html,/Active while the game is in front/);
  assert.equal((html.match(/aria-label="Configure /g)||[]).length,output.actions.length);
  assert.doesNotMatch(html,/Add exact|Remove action|Update desktop|Capture a physical|HID|Endpoint label/);
  h.update({name:'Edited',required:true});assert.equal(changes.at(-1).name,'Edited');assert.equal(changes.at(-1).required,true);
  h.scope('desktop-wide');assert.deepEqual(changes.at(-1).scope,{kind:'desktop-wide'});assert.equal(output.name,kind==='keyboard'?'Keyboard':'Mouse');
 }
});
test('keyboard inputs have no behavior settings and keep their destination unchanged',async()=>{
 const output=desktopBlock('keyboard','keys'),changes=[],view=await component('./ProfileDesktopEditor.svelte',never,'key-4');
 const {html,handlers:h}=view.render({output,change:value=>changes.push(plain(value))});assert.doesNotMatch(html,/Behavior|Repeat|Down \(ms\)|Delay \(ms\)|input-settings/);
 h.mode('repeat');h.mode('key-tap');assert.deepEqual(changes,[]);
 h.field('key',0);assert.match(h.state.error,/within the displayed limits/);assert.equal(changes.length,0);
 const blocked=await component('./ProfileDesktopEditor.svelte',never,'button-left');const blockedUI=blocked.render({output:desktopBlock('mouse','mouse'),change:()=>false});blockedUI.handlers.mode('mouse-click');assert.match(blockedUI.handlers.state.error,/Disconnect/);
 const throwing=await component('./ProfileDesktopEditor.svelte',never);const failed=throwing.render({output,change:()=>{throw Error('Edit failed');}});failed.handlers.update({name:'Rejected'});assert.match(failed.handlers.state.error,/Edit failed/);
});
test('mouse settings show only relevant pointer, wheel and button values',async()=>{
 const output=desktopBlock('mouse','mouse');
 for(const [id,expected,absent] of [['pointer-x',/Speed.*Curve/s,/Behavior/],['scroll-down',/Steps/,/Speed|Behavior/],['button-left',/Behavior/,/Steps|Speed/]]){
  const changes=[],view=await component('./ProfileDesktopEditor.svelte',never,id),{html,handlers:h}=view.render({output,change:value=>changes.push(plain(value))});assert.match(html,expected);assert.doesNotMatch(html,absent);
  if(id==='pointer-x'){h.field('speed',1500);assert.equal(changes[0].actions.find(item=>item.portId===id).action.speed,1500);h.field('curve',0);assert.match(h.state.error,/limits/);}
  if(id==='button-left'){h.mode('mouse-click');assert.equal(changes[0].actions[0].action.kind,'mouse-click');}
  if(id==='scroll-down'){h.field('vertical',-3);assert.equal(changes[0].actions.find(item=>item.portId===id).action.vertical,-3);}
 }
});
