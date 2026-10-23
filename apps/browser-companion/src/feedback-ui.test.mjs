import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import * as artifactApi from '../../../packages/widget-runtime/src/artifact.ts';
import * as playerApi from '../../../packages/widget-runtime/src/player.ts';
import * as panelApi from '../../../packages/widget-runtime/src/panel.ts';
import { createToggleSource } from '../../../packages/widget-compiler/src/seeds.ts';
import { compileWidget } from '../../../packages/widget-compiler/src/compiler.ts';
import { createBrowserFeedback } from './feedback.ts';
const bundle=await compileWidget(createToggleSource(),{motion:false,audio:false});
const publication=await panelApi.createCompiledPanel({id:'panel',title:'Toggle panel',viewports:[{pageId:'page',title:'Page',width:480,height:320}]},[{id:'toggle-instance',pageId:'page',label:'Toggle',bounds:{x:0,y:0,width:.5,height:.45},bundle}]);
function load(source,imports){const{js}=compile(source,{generate:'server'}),exports={};vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,structuredClone,performance:{now:()=>0},require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name in imports)return imports[name];throw Error(name);}});return exports.default;}

test('production compiled panel reserves finished-pixel surfaces without authoring layer markup',async()=>{
 const widgetSource=await readFile(new URL('../../../packages/companion-ui/src/CompiledWidget.svelte',import.meta.url),'utf8');
 const panelSource=await readFile(new URL('../../../packages/companion-ui/src/CompiledPanel.svelte',import.meta.url),'utf8');
 const Widget=load(widgetSource,{'../../widget-runtime/src/artifact.ts':artifactApi,'../../widget-runtime/src/player.ts':playerApi});
 const Panel=load(panelSource,{'./CompiledWidget.svelte':{default:Widget},'../../widget-runtime/src/panel.ts':panelApi});
 const html=render(Panel,{props:{publication}}).body;
 assert.match(html,/class="pixel-surface /);assert.ok(html.includes(bundle.artifactHash));assert.ok(html.includes('aria-label="Toggle"'));
 assert.equal(html.includes('<svg'),false);assert.equal(html.includes('data-layer-id'),false);assert.equal(html.includes('data-source-layer-id'),false);
});
test('requested Toggle pixels move immediately while confirmation stays independently unknown',()=>{
 const player=new playerApi.CompiledWidgetPlayer(bundle),before=artifactApi.decodeCompiledFrame(bundle.artifact,bundle.assets,player.frameId);
 const requested=player.dispatch({type:'event',event:'activate',nowMs:0});
 assert.equal(requested.state.values.requested,true);assert.equal(requested.state.values.quality,'unknown');assert.deepEqual(requested.effects.filter(e=>e.type==='emit'),[{type:'emit',port:'toggle',value:true}]);
 const after=artifactApi.decodeCompiledFrame(bundle.artifact,bundle.assets,player.frameId);assert.notDeepEqual(after,before);
 const disagree=player.dispatch({type:'event',event:'feedback',nowMs:10,feedback:{confirmed:{status:'fresh',value:false,provenance:'game-confirmed',sourceKind:'simulator'}}});
 assert.equal(disagree.state.values.requested,true);assert.equal(disagree.state.values.confirmed,false);assert.equal(disagree.state.values.quality,'fresh');assert.deepEqual(disagree.effects,[]);
 player.sample(1011);assert.equal(player.state.values.requested,true);assert.equal(player.state.values.quality,'stale');
 player.dispatch({type:'event',event:'activate',nowMs:1012});assert.equal(player.state.values.requested,false);
});
test('browser admitted feedback preserves game provenance and never fabricates requested confirmation',()=>{
 const feedback=createBrowserFeedback(),identity={clientId:'browser',sessionId:'session',generation:'1',controllerInstanceId:'instance',publicationHash:publication.publicationHash};
 const channel=publication.document.ui.feedbackWidgets[0],value={channelId:channel.channelId,outputId:channel.id,status:'fresh',value:true,provenance:'requested',sourceKind:'host',sourceAgeMs:0,unit:'boolean'};
 const frame={version:1,type:'feedback',...identity,sequence:'1',bindingGeneration:'1',mode:'snapshot',bindings:[{channelId:channel.channelId,outputId:channel.id}],values:[value]};
 feedback.accept(identity,frame,publication.document.ui,0);
 const player=new playerApi.CompiledWidgetPlayer(bundle),sample=feedback.snapshot()[channel.channelId];
 const event=panelApi.panelFeedbackEvents(publication,'toggle-instance',{[channel.channelId]:{status:sample.status,value:sample.value,sourceAgeMs:sample.sourceAgeMs,provenance:sample.provenance,sourceKind:sample.sourceKind}})[0];
 player.dispatch({type:'event',event:event.event,feedback:event.feedback,nowMs:0,suppressCues:true});assert.equal(player.state.values.quality,'unknown');assert.equal(player.state.values.requested,false);
 feedback.disconnect();assert.equal(feedback.snapshot()[channel.channelId].status,'disconnected');
});
test('production browser entry and compiled renderer have no source compiler or layer renderer dependency',async()=>{
 for(const path of ['./App.svelte','./compiled-cache.ts','../../../packages/companion-ui/src/CompiledPanel.svelte','../../../packages/companion-ui/src/CompiledWidget.svelte']){
  const source=await readFile(new URL(path,import.meta.url),'utf8');
  assert.doesNotMatch(source,/from\s+['"][^'"]*(?:widget-compiler|companion-designer|widget-presentation|WidgetView|PanelCanvas)/,path);
  assert.equal(source.includes('flightTemplate'),false,path);
 }
});
