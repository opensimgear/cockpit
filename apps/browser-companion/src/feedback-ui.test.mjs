import assert from'node:assert/strict';import test from'node:test';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import*as server from'svelte/internal/server';import*as svelte from'svelte';
import*as designer from'../../../packages/companion-designer/src/index.ts';import*as controls from'../../../packages/companion-ui/src/controller.ts';import*as presentation from'../../../packages/companion-ui/src/presentation.ts';import{createOrderedInputQueue}from'../../companion/src/lib/input-queue.ts';import*as feedbackApi from'./feedback.ts';import*as transport from'./transport.ts';
const identity={clientId:'browser',sessionId:'session',generation:'1',controllerInstanceId:'instance',publicationHash:'sha256:'+'a'.repeat(64)};
const panel=structuredClone(designer.flightTemplate),outputs=panel.ui.feedbackWidgets.filter(item=>['indicator','text'].includes(item.kind));
const values=outputs.map(output=>({channelId:output.channelId,outputId:output.id,status:'fresh',value:output.kind==='indicator'?true:'Altitude 12000 ft',provenance:'requested',sourceKind:'host',sourceAgeMs:10,unit:output.kind==='indicator'?'boolean':'text'}));
const reply={ok:true,type:'ack',feedback:{version:1,type:'feedback',...identity,sequence:'1',bindingGeneration:'1',mode:'snapshot',bindings:outputs.map(output=>({channelId:output.channelId,outputId:output.id})),values}};
const appSource=await readFile(new URL('./App.svelte',import.meta.url),'utf8'),canvasSource=await readFile(new URL('../../../packages/companion-ui/src/PanelCanvas.svelte',import.meta.url),'utf8');
function load(source,filename,imports,globals={}){const exports={};const{js}=compile(source,{filename,generate:'server'});vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,structuredClone,TextEncoder,performance:{now:()=>100},require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name in imports)return imports[name];throw Error(name);},...globals});return exports.default;}
function harness(action='',verified=panel){
 let handlers,clock=100;const performance={now:()=>clock};
 const PanelCanvas=load(canvasSource,'PanelCanvas.svelte',{'./controller.ts':controls,'./presentation.ts':presentation},{performance});
 const injected=appSource.replace('</script>',`credential={token:'a'.repeat(43),hostId:'host',companionId:'browser'};panel=seedPanel;pageId=panel.ui.pages[0].id;session=seedIdentity;active=true;epoch=7;consume(seedReply,7);${action};capture({consume,stop,resetFeedback,loadAssigned,bind(socket){connection=socket;owner=true;},get controls(){return controller.snapshot();},get feedback(){return feedback;}});</script>`);
 const App=load(injected,'App.svelte',{'@opensimgear/companion-ui':{PanelCanvas,...controls},'@opensimgear/companion-designer':designer,'../../companion/src/lib/companion-cache':{verifyPublishedPayload:async()=>({document:verified,assets:[],publicationHash:identity.publicationHash})},'../../companion/src/lib/input-queue':{createOrderedInputQueue},'./transport':transport,'./feedback':feedbackApi},{performance,document:{visibilityState:'visible',hasFocus:()=>true},seedPanel:panel,seedIdentity:identity,seedReply:reply,capture:v=>handlers=v,setClock:v=>clock=v});
 return{html:render(App).body,get handlers(){return handlers;}};
}
test('the actual browser App consumes accepted feedback into the actual shared renderer without creating control inputs',()=>{
 const h=harness();assert.match(h.html,/indicator-value[^>]* on/);assert.ok(h.html.includes('Altitude 12000 ft'));assert.ok(h.html.includes('requested · host'));assert.ok(h.html.includes('Resync visuals'));assert.ok(!h.html.includes('game-confirmed · simulator'));
 const before=structuredClone(h.handlers.feedback);h.handlers.consume({...reply,feedback:{...reply.feedback,sequence:'2',values:values.map(v=>({...v,value:typeof v.value==='boolean'?false:'Late'}))}},6);assert.deepEqual(h.handlers.feedback,before);
});
test('opening a verified different panel replaces the input controller before sending its fresh baseline',async()=>{
 const replacement=structuredClone(panel);replacement.ui.controls[0].id='custom-axis';for(const widget of replacement.presentation.widgets)widget.controlIds=widget.controlIds.map(id=>id===panel.ui.controls[0].id?'custom-axis':id);
 const h=harness("stop('Changing publication')",replacement),requests=[];
 const socket={request:async frame=>{requests.push(frame);return frame.type==='open'?{...identity,publication:{canonicalDocument:JSON.stringify(replacement)},publicationHash:identity.publicationHash}:{ok:true};}};h.handlers.bind(socket);
 await h.handlers.loadAssigned(socket,8,[{instanceId:'instance',publicationHash:identity.publicationHash,name:'Custom panel'}]);
 assert.equal(requests[0].type,'open');assert.equal(requests[1].type,'baseline');assert.equal(requests[1].values['custom-axis'],controls.fallbackValue(replacement.ui.controls[0]));assert.ok(!Object.hasOwn(h.handlers.controls.values,panel.ui.controls[0].id));assert.ok(Object.hasOwn(h.handlers.controls.values,'custom-axis'));assert.equal(h.handlers.controls.armed,true);
});
test('the actual browser clears live presentation on disconnect and lets stale values age through the shared renderer',()=>{
 const offline=harness("stop('Disconnected test')");assert.ok(!/indicator-value[^>]* on/.test(offline.html));assert.ok(offline.html.includes('Disconnected'));assert.ok(!offline.html.includes('Altitude 12000 ft'));
 const stale=harness('setClock(1500)');assert.ok(!/indicator-value[^>]* on/.test(stale.html));assert.ok(stale.html.includes('Stale · last Altitude 12000 ft'));assert.ok(stale.html.includes('requested (historical) · host'));
 const reset=harness('resetFeedback()');assert.ok(!/indicator-value[^>]* on/.test(reset.html));assert.ok(!reset.html.includes('Altitude 12000 ft'));
});
