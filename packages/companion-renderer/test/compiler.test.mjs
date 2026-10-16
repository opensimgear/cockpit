import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { gunzipSync } from "node:zlib";
import { publishDocument, newDocument, createDesignerHistory, applyDesignerCommand } from "../../companion-designer/src/index.ts";
import { SUNTON_PROFILE,WAVESHARE_PROFILE,compileResolvedPanel,compileLegacyPublication,createReferenceRenderer,createStarter,prepareRgba,prepareA4,prepareGlyph,packImages,packFonts,rgb565,blend565,rgb565Bytes,scalar,raw,variable,profileHash,verifyPackage,canonical } from "../src/index.ts";
const fontsJson=JSON.parse(await readFile(new URL("../assets/prepared-fonts.json",import.meta.url),"utf8"));
const license=await readFile(new URL("../assets/OFL.txt",import.meta.url),"utf8");
const fonts=fontsJson.fonts.map(font=>({...font,license,glyphs:font.glyphs.map(glyph=>({...glyph,coverage:Uint8Array.from(glyph.coverage)}))}));
const source={sourceUiId:"compiled-panel",sourcePublicationHash:"sha256:"+"1".repeat(64),semanticFingerprint:"sha256:"+"2".repeat(64),compilerId:"test-compiler",compilerVersion:"1.0.0"};
function simple(){
 const image=prepareRgba("color",2,2,Uint8Array.from([255,0,0,255,0,255,0,128,0,0,255,255,255,255,255,0]));
 const font={id:"font",lineHeight:6,ascent:4,glyphs:[32,45,46,48,49,63,65].map(codepoint=>prepareGlyph({codepoint,width:3,height:4,xOffset:0,yOffset:-4,advance:4,alpha:Uint8Array.from({length:12},(_,i)=>codepoint===32?0:i%3===1?255:128)}))};
 const semantics={version:1,controls:[{id:"fire",kind:"button",targetId:"target",targetIndex:0,fallback:0}],outputs:[{id:"power",channelId:"power-channel",kind:"number",min:0,max:100,unit:"%",maxTextBytes:0},{id:"lit",channelId:"lit-channel",kind:"boolean",min:0,max:1,unit:"",maxTextBytes:0}],interactions:[{id:"fire-owner",kind:"momentary",controlIds:["fire"]}]};
 const recipe={recipeVersion:1,id:"simple",images:[image.image,image.mask,prepareA4("press",44,44,new Uint8Array(44*44).fill(180))],fonts:[font],variables:[{id:"power",source:"feedback",outputId:"power"},{id:"lit",source:"feedback",outputId:"lit"},{id:"pressed",source:"pressed",interactionId:"fire-owner"},{id:"available",source:"available",outputId:"power"},{id:"fresh",source:"fresh",outputId:"power"},{id:"connected",source:"connected"}],
 pages:[{id:"main",background:0x0841,nodes:[
 {id:"button",kind:"rect",bounds:[8,32,44,44],color:0x1234},
 {id:"pressed",kind:"mask4",bounds:[8,32,44,44],maskId:"press",color:0x07ff,bindings:{visible:variable("pressed")}},
 {id:"image",kind:"image565",bounds:[72,32,2,2],imageId:"color"},
 {id:"mask",kind:"mask4",bounds:[76,32,2,2],maskId:"color-alpha",color:0xffff},
 {id:"colored-alpha",kind:"image565Mask4",bounds:[80,32,2,2],imageId:"color",maskId:"color-alpha"},
 {id:"label",kind:"text",bounds:[88,32,48,8],fontId:"font",text:"A?",color:0xffff},
 {id:"number",kind:"text",bounds:[88,48,48,8],fontId:"font",text:"0",maxTextBytes:8,precision:1,color:0xffff,bindings:{number:variable("power")}},
 {id:"line",kind:"line",bounds:[88,64,64,4],points:[[88,65],[150,65]],lineWidth:2,color:0x07e0},
 {id:"polyline",kind:"polyline",bounds:[88,72,64,8],points:[[88,72],[110,79],[150,72]],lineWidth:1,color:0xf800},
 {id:"moving",kind:"rect",bounds:[96,88,2,4],color:0xffff,bindings:{translateX:{kind:"lookup",value:variable("power"),points:[[0,0],[100*65536,12*65536]]}},timelines:[{property:"translateY",interpolation:"linear",repeat:false,keyframes:[[0,0],[1000,2*65536]]}]},
 {id:"local",kind:"rect",bounds:[160,32,8,8],color:0xffff,bindings:{visible:{kind:"op",op:"and",args:[variable("available"),{kind:"op",op:"and",args:[variable("fresh"),variable("connected")]}]}}}
 ]}],hits:[{interactionId:"fire-owner",nodeId:"button"}]};
 return{semantics,recipe};
}
const state=(overrides={})=>({page:0,connected:true,timeMs:0,feedback:[{type:"number",available:true,fresh:true,value:50},{type:"boolean",available:true,fresh:true,value:true}],pressed:[false],...overrides});

test("typed artwork and glyph preparation preserve nibble order, transparent alpha and explicit coverage bounds",async()=>{
 assert.equal(rgb565(255,255,255),65535);assert.equal(blend565(65535,0,15),65535);assert.equal(blend565(65535,0,0),0);
 assert.deepEqual([...prepareA4("odd",3,1,Uint8Array.from([255,128,255])).pixels],[0xf8,0xf0]);
 assert.throws(()=>prepareA4("bad",1,1,new Uint8Array()));assert.throws(()=>prepareRgba("bad",1,1,new Uint8Array()));
 assert.throws(()=>prepareA4("bad",0,1,new Uint8Array()));
 const zero=prepareGlyph({codepoint:32,width:0,height:0,xOffset:0,yOffset:0,advance:2,alpha:new Uint8Array()});assert.equal(zero.coverage.length,0);assert.equal(Object.hasOwn(zero,"alpha"),false);
 assert.throws(()=>prepareGlyph({codepoint:63,width:1,height:1,xOffset:0,yOffset:0,advance:1,alpha:new Uint8Array()}));
 const a=prepareRgba("first",1,1,Uint8Array.from([255,0,0,255])).image,b={...a,id:"alias"};
 const packed=await packImages([a,b]);assert.equal(packed.indexes.get("alias"),0);assert.equal((await packImages([])).bytes,undefined);assert.equal(packFonts([]).bytes,undefined);
 await assert.rejects(()=>packImages([a,a]),/Duplicate/);await assert.rejects(()=>packImages([{...a,format:"png"}]),/format/);await assert.rejects(()=>packImages([{...a,pixels:new Uint8Array()}]),/byte/);
 const font=simple().recipe.fonts[0];
 for(const bad of[{...font,glyphs:[]},{...font,glyphs:[font.glyphs[0]]},{...font,lineHeight:0},{...font,glyphs:[...font.glyphs,font.glyphs[0]]},{...font,glyphs:font.glyphs.map((g,i)=>i?g:{...g,advance:0})},{...font,glyphs:font.glyphs.map((g,i)=>i?g:{...g,coverage:new Uint8Array()})}])assert.throws(()=>packFonts([bad]));
 assert.throws(()=>packFonts([font,font]),/Duplicate/);
 const licensed=packFonts([{...font,license:"Test source license\n"}]);assert.ok(new TextDecoder().decode(licensed.bytes).includes("OSG FONT LICENSE"));
});

test("resolved compiler lowers all primitives, local sources, proven expressions, timelines and spatial coverage",async()=>{
 const {semantics,recipe}=simple(),compiled=await compileResolvedPanel(source,semantics,recipe,SUNTON_PROFILE);
 assert.equal(compiled.manifest.counts.nodes,11);assert.ok(compiled.manifest.requires.includes("scene.local-state"));assert.ok(compiled.manifest.requires.includes("scene.image565-mask4"));
 assert.equal(compiled.manifest.resources.internalRamBytes,24576);assert.ok(compiled.manifest.resources.snapshotBytes>=8192);
 const renderer=await createReferenceRenderer(compiled,SUNTON_PROFILE),first=renderer.update(state());
 assert.equal(first.properties[9].x,6);assert.equal(first.properties[9].y,0);assert.equal(first.properties[10].visible,true);
 assert.equal(renderer.hitTest(9,33),0);assert.equal(renderer.hitTest(0,0),null);
 const pressed=renderer.update(state({timeMs:500,pressed:[true]}));assert.notDeepEqual(rgb565Bytes(first.frame),rgb565Bytes(pressed.frame));assert.equal(pressed.properties[9].y,1);
 const held=renderer.update(state({timeMs:3000}));assert.equal(held.properties[9].y,2);
 const unavailable=renderer.update(state({timeMs:3100,feedback:[{type:"number",available:false,fresh:false},{type:"boolean",available:true,fresh:true,value:true}]}));assert.equal(unavailable.properties[6].visible,false);assert.equal(unavailable.properties[10].visible,false);
 const bad=renderer.update(state({timeMs:3200,feedback:[{type:"number",available:true,fresh:true,value:NaN}]}));assert.equal(bad.properties[6].visible,false);
 const overflow=renderer.update(state({timeMs:3300,feedback:[{type:"number",available:true,fresh:true,value:1e9}]}));assert.equal(overflow.properties[6].visible,false);
 const wrong=renderer.update(state({timeMs:3400,feedback:[{type:"boolean",available:true,fresh:true,value:true}]}));assert.equal(wrong.properties[6].visible,false);
 assert.equal(renderer.update(state({timeMs:3500,connected:false})).elapsedMs,0);assert.equal(renderer.hitTest(9,33),null);
 assert.equal(renderer.update(state({timeMs:4000})).elapsedMs,0);assert.throws(()=>renderer.update(state({timeMs:3999})),/monotonic/);
});

test("compiler fails closed on unsupported identities, references, integer conversion and unprovable data",async()=>{
 const base=simple();
 for(const mutation of[
 r=>r.recipeVersion=2,r=>r.pages=[],r=>r.variables.push({...r.variables[0]}),r=>r.pages[0].nodes.push({...r.pages[0].nodes[0]}),
 r=>r.pages[0].nodes[0].kind="svg",r=>r.pages[0].nodes[0].bounds=[1e9,32,44,44],r=>r.pages[0].nodes[2].imageId="missing",
 r=>r.variables[0].outputId="missing",r=>r.variables[2].interactionId="missing",r=>r.variables[5].source="script",
 r=>r.pages[0].nodes[0].bindings={visible:{kind:"variable",id:"missing"}},
 r=>r.pages[0].nodes[0].bindings={visible:{kind:"op",op:"not",args:[]}},
 r=>r.pages[0].nodes[0].bindings={visible:{kind:"op",op:"script",args:[]}},
 r=>r.pages[0].nodes[9].bindings.translateX.points=[[0,0]],
 r=>r.pages[0].nodes[9].bindings.translateX.points=[[0,0],[0,65536]],
 r=>r.pages[0].nodes[9].bindings.translateX={kind:"op",op:"divide",args:[scalar(1),scalar(0)]},
 r=>r.pages[0].nodes[0].bindings={number:scalar(1)},r=>r.hits[0].nodeId="missing",
 ]){
   const recipe=structuredClone(base.recipe);mutation(recipe);await assert.rejects(()=>compileResolvedPanel(source,base.semantics,recipe,SUNTON_PROFILE));
 }
 const changed=structuredClone(base.semantics);changed.interactions[0].controlIds=[];await assert.rejects(()=>compileResolvedPanel(source,changed,base.recipe,SUNTON_PROFILE),/Unowned|arity/);
 const nav=structuredClone(base.semantics);nav.interactions.push({id:"next",kind:"navigation",controlIds:[],destinationPage:"secondary"});
 const recipe=structuredClone(base.recipe);recipe.pages.push({id:"secondary",background:0,nodes:[{id:"nav",kind:"rect",bounds:[8,32,44,44],color:0x1234}]});recipe.hits.push({interactionId:"next",nodeId:"nav"});
 const compiled=await compileResolvedPanel(source,nav,recipe,SUNTON_PROFILE),renderer=await createReferenceRenderer(compiled,SUNTON_PROFILE);
 renderer.update(state());assert.equal(renderer.update(state({timeMs:100,page:1})).elapsedMs,0);assert.equal(renderer.hitTest(9,33),1);
});

test("legacy compiler derives authority only from source publication and rejects unsupported behavior",async()=>{
 const starter=createStarter(SUNTON_PROFILE,"a",fonts),published=await publishDocument(starter.document,[]);
 const compiled=await compileLegacyPublication(published,starter.recipe,SUNTON_PROFILE);
 assert.deepEqual(compiled.semantics.controls.map(c=>c.id),published.document.ui.controls.map(c=>c.id));assert.deepEqual(compiled.semantics.interactions.map(i=>i.id),["laser-button","stop-button"]);
 assert.deepEqual(compiled.semantics.outputs.map(o=>o.id),published.document.ui.feedbackWidgets.map(o=>o.id));
 await assert.rejects(()=>compileLegacyPublication({...published,publicationHash:"sha256:"+"0".repeat(64)},starter.recipe,SUNTON_PROFILE),/identity/);
 const toggle=structuredClone(starter.document);toggle.presentation.widgets[0].config.buttonMode="toggle";await assert.rejects(()=>publishDocument(toggle,[]).then(p=>compileLegacyPublication(p,starter.recipe,SUNTON_PROFILE)),/momentary/);
 const h=applyDesignerCommand(createDesignerHistory(newDocument()),{type:"add-widget",kind:"text"});await assert.rejects(()=>publishDocument(h.present,[]).then(p=>compileLegacyPublication(p,starter.recipe,SUNTON_PROFILE)),/text feedback/);
 const fractional=structuredClone(starter.document);for(const widget of fractional.presentation.widgets)if(widget.kind==="numeric"||widget.kind==="gauge"){widget.config.min=0.1;widget.config.max=0.9;}
 await assert.rejects(()=>publishDocument(fractional,[]).then(p=>compileLegacyPublication(p,starter.recipe,SUNTON_PROFILE)),/exactly representable as Q16.16/);
 for(const widget of fractional.presentation.widgets)if(widget.kind==="numeric"||widget.kind==="gauge"){widget.config.min=0.5;widget.config.max=99.5;}
 assert.ok(await publishDocument(fractional,[]).then(p=>compileLegacyPublication(p,starter.recipe,SUNTON_PROFILE)));
 const axes=applyDesignerCommand(createDesignerHistory(newDocument()),{type:"add-widget",kind:"slider"});await assert.rejects(()=>publishDocument(axes.present,[]).then(p=>compileLegacyPublication(p,starter.recipe,SUNTON_PROFILE)),/momentary/);
});

test("both board A/B/C packages recompile deterministically and reference raster states match exact shared goldens",async()=>{
 const cases=JSON.parse(await readFile(new URL("./poc/cases.json",import.meta.url),"utf8")),fingerprints=new Map();
 for(const entry of cases.cases){
   const [board,revision]=entry.package.split("/"),profile=board==="sunton"?SUNTON_PROFILE:WAVESHARE_PROFILE,dir=new URL("./poc/"+entry.package+"/",import.meta.url);
   const starter=createStarter(profile,revision.toLowerCase(),fonts),publication=await publishDocument(starter.document,[]),compiled=await compileLegacyPublication(publication,starter.recipe,profile);
   const expected=JSON.parse(await readFile(new URL("identity.json",dir),"utf8"));
   assert.equal(compiled.packageHash,expected.packageHash);assert.equal(compiled.recipeHash,expected.recipeHash);
   assert.equal(canonical(compiled.semantics),canonical(JSON.parse(await readFile(new URL("semantics.json",dir),"utf8"))));
   if(fingerprints.has(board))assert.equal(publication.capabilityFingerprint,fingerprints.get(board));else fingerprints.set(board,publication.capabilityFingerprint);
   const diskObjects=new Map();for(const object of compiled.manifest.objects)diskObjects.set(object.kind,new Uint8Array(await readFile(new URL(object.kind+".bin",dir))));
   await verifyPackage(new Uint8Array(await readFile(new URL("manifest.json",dir))),diskObjects,profile,compiled.packageHash);
   const renderer=await createReferenceRenderer(compiled,profile);
   for(const frame of entry.frames){
     const state={...frame.state,feedback:frame.state.feedback.map(f=>({...f,type:f.type===1?"boolean":"number",value:f.type===1?!!f.value:f.value}))};
     const result=renderer.update(state),compressed=await readFile(new URL(frame.rgb565,dir));assert.equal(compressed.readUInt32LE(4),0,"deterministic gzip timestamp");
     assert.deepEqual(rgb565Bytes(result.frame),new Uint8Array(gunzipSync(compressed)),entry.package+"/"+frame.name);
     assert.equal(result.elapsedMs,frame.elapsedMs);
     for(const hit of frame.hitTests)assert.equal(renderer.hitTest(hit.x,hit.y),hit.expected);
   }
 }
 const header=await readFile(new URL("../../../apps/companion-esp32/render/profiles.generated.h",import.meta.url),"utf8");assert.ok(header.includes(await profileHash(SUNTON_PROFILE)));assert.ok(header.includes(await profileHash(WAVESHARE_PROFILE)));
});

test("package-only revision C pulses each held button and leaves unpressed buttons still within existing profiles",async()=>{
 for(const [board,profile] of [["sunton",SUNTON_PROFILE],["waveshare",WAVESHARE_PROFILE]]){
   const dir=new URL("./poc/"+board+"/C/",import.meta.url),prior=new URL("./poc/"+board+"/B/",import.meta.url);
   const manifestBytes=new Uint8Array(await readFile(new URL("manifest.json",dir))),manifest=JSON.parse(new TextDecoder().decode(manifestBytes));
   const identity=JSON.parse(await readFile(new URL("identity.json",dir),"utf8")),bManifest=JSON.parse(await readFile(new URL("manifest.json",prior),"utf8"));
   const starter=createStarter(profile,"c",fonts),objects=new Map();
   for(const object of manifest.objects)objects.set(object.kind,new Uint8Array(await readFile(new URL(object.kind+".bin",dir))));
   const packed={manifest,manifestBytes,objects,packageHash:identity.packageHash};
   const renderer=await createReferenceRenderer(packed,profile),frames=JSON.parse(await readFile(new URL("states.json",dir),"utf8")).frames,rendered=new Map();
   for(const frame of frames){
     const state={...frame.state,feedback:frame.state.feedback.map(f=>({...f,type:f.type===1?"boolean":"number",value:f.type===1?!!f.value:f.value}))};
     rendered.set(frame.name,renderer.update(state));
   }
   assert.equal(manifest.counts.timelines,3);assert.ok(manifest.counts.timelines<=profile.limits.timelines);
   assert.equal(manifest.resources.maxDirtyPixels,bManifest.resources.maxDirtyPixels);
   assert.equal(manifest.resources.maxLayersPerTile,bManifest.resources.maxLayersPerTile);
   assert.equal(manifest.semanticDescriptorHash,bManifest.semanticDescriptorHash);
   assert.equal(manifest.objects.find(o=>o.kind==="images").sha256,bManifest.objects.find(o=>o.kind==="images").sha256);
   for(const object of manifest.objects)assert.equal(object.version,1);
   for(const [name,bright,dim,released] of [["laser","pressed","held","confirmed"],["stop","stop-pressed","stop-held","stop-released"]]){
     const index=starter.recipe.pages[0].nodes.findIndex(node=>node.id===name+"-pressed");
     const node=starter.recipe.pages[0].nodes[index],timeline=node.timelines[0];
     assert.equal(timeline.repeat,true);assert.equal(timeline.property,"color");assert.equal(timeline.interpolation,"step");
     assert.deepEqual(timeline.keyframes.map(k=>k[0]),[0,400,800]);
     assert.equal(rendered.get(bright).properties[index].visible,true);
     assert.equal(rendered.get(dim).properties[index].visible,true);
     assert.notEqual(rendered.get(bright).properties[index].color,rendered.get(dim).properties[index].color);
     assert.equal(rendered.get(released).properties[index].visible,false);
     const [x,y,w,h]=node.bounds;let changed=0;
     for(let py=y;py<y+h;++py)for(let px=x;px<x+w;++px)changed+=Number(rendered.get(bright).frame[py*profile.width+px]!==rendered.get(dim).frame[py*profile.width+px]);
     assert.ok(changed>w*h/2,board+"/"+name+" held pulse must be conspicuous");
   }
   // The two released frames have equal feedback; invisible color timelines
   // cannot animate either button or alter the rest of the frame.
   assert.deepEqual(rendered.get("reconnected").frame,rendered.get("stop-released").frame);
   const live=frames.find(f=>f.name==="live").state;
   const idle=createReferenceRenderer(packed,profile).then(r=>{
     const typed={...live,feedback:live.feedback.map(f=>({...f,type:f.type===1?"boolean":"number",value:f.type===1?!!f.value:f.value}))};
     const first=r.update({...typed,timeMs:100}).frame,second=r.update({...typed,timeMs:600}).frame;
     assert.deepEqual(first,second);
   });
   await idle;
 }
});