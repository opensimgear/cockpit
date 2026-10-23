import assert from'node:assert/strict';import test from'node:test';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import*as server from'svelte/internal/server';import*as svelte from'svelte';
import*as designer from'../../../packages/companion-designer/src/index.ts';import*as controls from'../../../packages/companion-ui/src/controller.ts';import*as presentation from'../../../packages/companion-ui/src/presentation.ts';import{createOrderedInputQueue}from'../../companion/src/lib/input-queue.ts';import*as feedbackApi from'./feedback.ts';import*as transport from'./transport.ts';
import*as widgets from'../../../packages/companion-ui/src/widget-presentation.ts';import*as fonts from'../../../packages/companion-ui/src/widget-fonts.ts';
const identity={clientId:'browser',sessionId:'session',generation:'1',controllerInstanceId:'instance',publicationHash:'sha256:'+'a'.repeat(64)};
const panel=structuredClone(designer.flightTemplate),outputs=panel.ui.feedbackWidgets.filter(item=>['indicator','text'].includes(item.kind));
const values=outputs.map(output=>({channelId:output.channelId,outputId:output.id,status:'fresh',value:output.kind==='indicator'?true:'Altitude 12000 ft',provenance:'requested',sourceKind:'host',sourceAgeMs:10,unit:output.kind==='indicator'?'boolean':'text'}));
const reply={ok:true,type:'ack',feedback:{version:1,type:'feedback',...identity,sequence:'1',bindingGeneration:'1',mode:'snapshot',bindings:outputs.map(output=>({channelId:output.channelId,outputId:output.id})),values}};
const appSource=await readFile(new URL('./App.svelte',import.meta.url),'utf8'),canvasSource=await readFile(new URL('../../../packages/companion-ui/src/PanelCanvas.svelte',import.meta.url),'utf8');
const widgetSource=await readFile(new URL('../../../packages/companion-ui/src/WidgetView.svelte',import.meta.url),'utf8'),groupSource=await readFile(new URL('../../../packages/companion-ui/src/GroupOutline.svelte',import.meta.url),'utf8');
function load(source,filename,imports,globals={}){const exports={};const{js}=compile(source,{filename,generate:'server'});const url=new URL(filename,filename==='App.svelte'?import.meta.url:new URL('../../../packages/companion-ui/src/',import.meta.url));vm.runInNewContext(ts.transpileModule(js.code.replaceAll('import.meta.url',JSON.stringify(url.href)),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,structuredClone,TextEncoder,URL,performance:{now:()=>100},require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name in imports)return imports[name];throw Error(name);},...globals});return exports.default;}
function harness(action='',verified=panel,seedAssets={},seedReply=reply){
 let handlers,clock=100;const performance={now:()=>clock};
 const WidgetView=load(widgetSource,'WidgetView.svelte',{'./presentation.ts':presentation,'./widget-presentation.ts':widgets,'./widget-fonts.ts':fonts},{performance});
 const GroupOutline=load(groupSource,'GroupOutline.svelte',{'./widget-fonts.ts':fonts},{performance});
 const PanelCanvas=load(canvasSource,'PanelCanvas.svelte',{'@opensimgear/companion-designer':designer,'./controller.ts':controls,'./presentation.ts':presentation,'./WidgetView.svelte':{default:WidgetView},'./GroupOutline.svelte':{default:GroupOutline},'./widget-presentation.ts':widgets},{performance});
 const injected=appSource.replace('</script>',`credential={token:'a'.repeat(43),hostId:'host',companionId:'browser'};panel=seedPanel;assets=seedAssets;pageId=panel.ui.pages[0].id;session=seedIdentity;active=true;epoch=7;consume(seedReply,7);${action};capture({consume,stop,resetFeedback,loadAssigned,bind(socket){connection=socket;owner=true;},get controls(){return controller.snapshot();},get feedback(){return feedback;}});</script>`);
 const App=load(injected,'App.svelte',{'@opensimgear/companion-ui':{PanelCanvas,...controls},'@opensimgear/companion-designer':designer,'../../companion/src/lib/companion-cache':{verifyPublishedPayload:async()=>({document:verified,assets:[],publicationHash:identity.publicationHash})},'../../companion/src/lib/input-queue':{createOrderedInputQueue},'./transport':transport,'./feedback':feedbackApi},{performance,document:{visibilityState:'visible',hasFocus:()=>true},seedPanel:verified,seedAssets,seedIdentity:identity,seedReply,capture:v=>handlers=v,setClock:v=>clock=v});
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

test('the actual browser renders image widgets without unsupported-widget errors or extra inputs',()=>{
 const document=structuredClone(panel);document.ui.assets.push({id:'picture',mime:'image/png',sizeBytes:1,sha256:'a'.repeat(64)});
 document.presentation.widgets.push({id:'image-widget',pageId:document.ui.pages[0].id,kind:'image',label:'Picture',bounds:{x:.1,y:.1,width:.2,height:.2},controlIds:[],feedbackIds:[],config:{assetId:'picture'}});
 const h=harness('',document,{picture:'data:image/png;base64,aGVsbG8='});
 assert.ok(h.html.includes('image-content'));assert.ok(h.html.includes('data:image/png;base64,aGVsbG8='));assert.ok(!h.html.includes('Unsupported widget'));
 assert.equal(Object.keys(h.handlers.controls.values).length,panel.ui.controls.length);
});

test('shared artwork clips include complete shape strokes while preserving text and image crops',()=>{
 const WidgetView=load(widgetSource,'WidgetView.svelte',{'./presentation.ts':presentation,'./widget-presentation.ts':widgets,'./widget-fonts.ts':fonts});
 const definition=designer.createWidgetCatalog().find(d=>d.id==='osg-capsule-toggle-horizontal');
 for(const state of ['normal','pressed','active']){
  const layers=designer.resolveWidgetLayers(definition,{},state);
  layers.push(...['ellipse','line','text','image','gauge'].map((kind,index)=>({id:`crop-${kind}`,label:kind,kind,bounds:{x:.2,y:.25,width:.4,height:.5},...(index<2?{strokeWidth:10}:{}),...(kind==='text'?{text:'TEST',font:'sans',fontSize:24,align:'center'}:{}),...(kind==='gauge'?{orientation:'radial',outputPort:'value',min:0,max:1}:{}),...(kind==='image'?{assetId:'picture'}:{})})));
  const html=render(WidgetView,{props:{definition,layers,state}}).body;
  const crop=id=>{const match=html.match(new RegExp(`<clipPath id="[^"]*-${id}"><rect x="([^"]+)" y="([^"]+)" width="([^"]+)" height="([^"]+)"`));assert.ok(match,id);return match.slice(1).map(Number);};
  const track=layers.find(l=>l.id==='track'),half=Number(track.strokeWidth)/2;
  assert.deepEqual(crop('track'),[36-half,56-half,128+half*2,56+half*2]);
  for(const kind of ['ellipse','line'])assert.deepEqual(crop(`crop-${kind}`),[35,27,90,74]);
  for(const kind of ['text','image'])assert.deepEqual(crop(`crop-${kind}`),[40,32,80,64]);
  assert.deepEqual(crop('crop-gauge'),[37,29,86,70]);
 }
});

test('browser capsule artwork keeps the requested OFF thumb and labels unconfirmed feedback separately',()=>{
 for(const definition of designer.createWidgetCatalog().filter(d=>d.id.startsWith('osg-capsule-toggle-'))){
  const document=designer.applyDesignerCommand(designer.createDesignerHistory(designer.newDocument()),{type:'add-component',definition}).present;
  const unknown=harness('',document,{},{ok:true,type:'ack'}).html;
  assert.ok(unknown.includes('data-layer-id="off-thumb"'),definition.id);
  assert.ok(!unknown.includes('data-layer-id="on-thumb"'),definition.id);
  assert.ok(unknown.includes('Game: unknown'),definition.id);
  const offline=harness("stop('Disconnected test')",document,{},{ok:true,type:'ack'}).html;
  assert.ok(offline.includes('data-layer-id="off-thumb"'),definition.id);
  assert.ok(!offline.includes('data-layer-id="on-thumb"'),definition.id);
 }
});

test('the actual browser App renders every shared catalog family through the production composition renderer',async()=>{
 const{document}=JSON.parse(await readFile(new URL('../../../examples/companion/shared-systems-complete.bundle.json',import.meta.url),'utf8'));
 const h=harness('controller.setDocument(panel)',document,{},{ok:true,type:'ack'});
 const catalogIds=designer.createWidgetCatalog().map(definition=>definition.id).sort();
 const components=document.presentation.widgets.filter(widget=>widget.kind==='component'&&widget.pageId===document.ui.pages[0].id);
 assert.deepEqual(components.map(widget=>widget.config.definitionId).sort(),catalogIds,'the fixture must exercise every shared catalog family');
 assert.deepEqual([...h.html.matchAll(/data-definition-id="([^"]+)"/g)].map(match=>match[1]).sort(),catalogIds,'every catalog family must reach the production renderer');
 assert.equal((h.html.match(/class="widget-artwork\b/g)??[]).length,components.length);
 assert.ok(h.html.includes('data-layer-id="ring"'));assert.ok(h.html.includes('data-layer-id="value"'));
 assert.equal(Object.keys(h.handlers.controls.values).length,document.ui.controls.length);
});
