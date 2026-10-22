import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {canonical,normalizeToPortableUi,renderCompatibility,validateDesignerDocument} from '../packages/companion-designer/src/index.ts';

const catalog=JSON.parse(await readFile('examples/companion/shared-systems.bundle.json','utf8')).document;
const legacy=JSON.parse(JSON.parse(await readFile('apps/companion-esp32/test/live-publication.json','utf8')).canonicalDocument);
const profiles=JSON.parse(await readFile('apps/companion-esp32/test/render-profiles.json','utf8')).valid;
function single(index=0){
  const doc=structuredClone(catalog),widget=doc.presentation.widgets[index];
  doc.presentation.widgets=[widget];doc.presentation.definitions=doc.presentation.definitions.filter(def=>def.id===widget.config.definitionId);
  doc.ui.controls=doc.ui.controls.filter(control=>widget.controlIds.includes(control.id));doc.ui.feedbackWidgets=doc.ui.feedbackWidgets.filter(output=>widget.feedbackIds.includes(output.id));
  doc.bindings=doc.bindings.filter(binding=>widget.controlIds.includes(binding.controlId));doc.ui=normalizeToPortableUi(doc);return doc;
}
const cases=[];
function add(name,doc,profile=1){
  doc=structuredClone(doc);
  doc.ui=normalizeToPortableUi(doc);const checked=validateDesignerDocument(doc);assert.ok(checked.ok,checked.error);
  const report=structuredClone(profiles[profile]),issues=renderCompatibility(doc,report);
  cases.push({name,profile,document:doc,issues});
}
add('rich catalog',structuredClone(catalog));add('compact catalog rejects retained budgets',structuredClone(catalog),0);
add('compact reusable switch',single(),0);add('rich legacy panel',structuredClone(legacy));
let doc=single();doc.presentation.viewports[0].width=1600;doc.presentation.viewports[0].height=960;add('letterboxed authored canvas',doc);
doc=single();doc.presentation.widgets[0].bounds.width=.05;add('small interactive hit area',doc);
doc=single();doc.presentation.widgets[0].bounds.width=.22;doc.presentation.definitions[0].hitArea.width=.1;add('small local hit area',doc);
doc=single();doc.presentation.viewports[0].safeInsets={left:.2,right:.2,top:.2,bottom:.2};add('inset geometry',doc);
doc=single();doc.presentation.definitions[0].layers.find(layer=>layer.kind==='text').fontSize=49;add('compact font budget',doc,0);add('rich font budget accepts same source',structuredClone(doc));
doc=single();doc.presentation.definitions[0].layers.find(layer=>layer.kind==='text').states={pressed:{text:'😀'}};add('alternate-state unsupported glyph',doc);
doc=single();doc.presentation.widgets[0].config.properties.label='ÆTHER';add('portable sans glyph',doc);
doc=single();doc.presentation.widgets[0].config.properties.label='Ж';add('sans glyph missing',doc);doc.presentation.definitions[0].layers.find(layer=>layer.kind==='text').font='mono';add('same glyph supported by mono',doc);
doc=single();doc.presentation.definitions[0].layers.find(layer=>layer.kind==='text').states={normal:{visible:false},pressed:{visible:true,text:'😀'}};add('visible alternate state is checked',doc);
doc=single();doc.presentation.definitions[0].layers.find(layer=>layer.kind==='text').visible=false;doc.presentation.widgets[0].config.properties.label='😀';add('hidden text does not reject',doc);
doc=single();doc.presentation.groups=[{id:'group-frame',pageId:doc.ui.pages[0].id,label:'😀'}];doc.presentation.widgets[0].groupId='group-frame';add('group glyph',doc);
doc=single();doc.presentation.groups=[{id:'empty-group',pageId:doc.ui.pages[0].id,label:'😀'}];add('unused group does not draw',doc);
doc=single();doc.ui.assets=[{id:'image-a',sha256:'a'.repeat(64),sizeBytes:100,mime:'image/png'},{id:'image-alias',sha256:'a'.repeat(64),sizeBytes:100,mime:'image/png'}];
doc.presentation.definitions[0].layers.push({id:'picture',label:'Picture',kind:'image',bounds:{x:0,y:0,width:1,height:1},assetId:'image-a'});add('prepared layer image',doc);
doc.presentation.definitions[0].layers.at(-1).bounds.width=.001;add('zero raster pixels',doc);
doc=single();doc.presentation.viewports[0].backgroundAssetId='background';doc.ui.assets=[{id:'background',sha256:'b'.repeat(64),sizeBytes:100,mime:'image/png'}];add('unsupported viewport background',doc);
doc=structuredClone(legacy);doc.presentation.widgets.find(widget=>widget.kind==='button').config.buttonMode='toggle';add('unsupported legacy interaction',doc);
doc=structuredClone(legacy);doc.presentation.widgets[0].label='😀';add('unsupported legacy glyph',doc);
doc=structuredClone(legacy);doc.presentation.widgets.find(widget=>widget.kind==='numeric').config.unit='x'.repeat(33);add('legacy unit byte budget',doc);
doc=single(7);doc.presentation.widgets[0].bounds={x:0,y:0,width:.0001,height:.0001};add('zero component pixels',doc);
function images(count,aliases=false){
  const result=single(7),template=result.presentation.widgets[0];result.presentation.definitions=[];result.ui.controls=[];result.ui.feedbackWidgets=[];result.bindings=[];
  result.ui.assets=Array.from({length:count},(_,index)=>({id:`asset-${index}`,sha256:(aliases?'a':index.toString(16)).repeat(64),sizeBytes:100,mime:'image/png'}));
  result.presentation.widgets=result.ui.assets.map((asset,index)=>({...template,id:`image-${index}`,kind:'image',bounds:{x:0,y:0,width:1,height:1},controlIds:[],feedbackIds:[],config:{assetId:asset.id}}));return result;
}
add('full-screen image',images(1));add('prepared image aliases share bytes',images(4,true));add('unique rasters exceed pixel budget',images(2));
doc=images(1);doc.presentation.viewports[0].width=320;doc.presentation.viewports[0].height=320;add('square raster fits',doc);
doc=single(7);const picture={id:'tiny-picture',label:'Tiny picture',kind:'image',bounds:{x:0,y:0,width:.01,height:.01},assetId:'asset-0'};
doc.ui.assets=[{id:'asset-0',sha256:'a'.repeat(64),sizeBytes:100,mime:'image/png'}];doc.presentation.definitions[0].layers=Array.from({length:32},(_,index)=>({...picture,id:`layer-${index}`}));
doc.presentation.widgets.push({...structuredClone(doc.presentation.widgets[0]),id:'second-image-component'});add('prepared reference budget',doc);
doc=single(7);doc.presentation.widgets=Array.from({length:33},(_,index)=>({...structuredClone(doc.presentation.widgets[0]),id:`frame-${index}`}));add('rich instance budget',doc);add('compact instance budget',doc,0);
doc=single();doc.presentation.definitions[0].layers=Array.from({length:25},(_,index)=>({...structuredClone(doc.presentation.definitions[0].layers[0]),id:`layer-${index}`}));add('unused scratch layer budget',doc,0);
doc=single();doc.bindings=[];add('incomplete logical binding',doc);
doc=images(1);delete doc.presentation.widgets[0].config.assetId;add('empty image asset',doc);
doc=single();doc.presentation.definitions[0].layers.find(layer=>layer.kind==='text').states={normal:{opacity:0},active:{opacity:1,text:'😀'}};add('transparent alternate text',doc);

// Keep source identity in the oracle to prove target checks never rewrite it.
for(const item of cases){item.canonicalDocument=canonical(item.document);delete item.document;}
const path='apps/companion-esp32/test/widget-compatibility.json',text=JSON.stringify({formatVersion:1,profiles,cases},null,2)+'\n';
if(process.argv.includes('--check'))assert.equal((await readFile(path,'utf8')).replaceAll('\r\n','\n'),text);
else await writeFile(path,text);
console.log(`${cases.length} shared target compatibility cases; ${cases.filter(item=>!item.issues.length).length} accepted unchanged.`);
