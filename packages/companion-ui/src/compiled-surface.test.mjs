import test from 'node:test';
import assert from 'node:assert/strict';
import {compileWidget} from '../../widget-compiler/src/index.ts';
import {createCompiledPanel} from '../../widget-runtime/src/panel.ts';
import {prepareCompiledSurface,prepareCompiledSurfaces,disposeCompiledSurfaces} from './compiled-surface.ts';
import {tinySource} from '../../widget-compiler/test/fixtures.mjs';
globalThis.ImageData??=class {constructor(data,width,height){this.data=data;this.width=width;this.height=height;}};
function canvas(fault){return{width:0,height:0,painted:undefined,getContext(){if(fault==='context')return null;return{putImageData:image=>{if(fault==='allocation')throw new RangeError('Canvas backing store allocation failed');this.painted=Uint8ClampedArray.from(image.data);}};}};}
const bundle=await compileWidget(tinySource());

test('candidate surfaces own the actual decoded pixels and painted Canvas before activation',async()=>{
 const target=canvas(),prepared=await prepareCompiledSurface(bundle,{createCanvas:()=>target,nowMs:25});assert.equal(prepared.canvas,target);assert.equal(prepared.player.state.nowMs,25);assert.equal(prepared.frameId,prepared.player.frameId);assert.equal(target.width,32);assert.equal(target.painted.length,4096);assert.deepEqual(target.painted.slice(0,4),Uint8ClampedArray.from([255,0,0,255]));prepared.dispose();prepared.dispose();assert.equal(target.width,0);assert.throws(()=>prepared.pixels.decode(prepared.frameId),/disposed/);
});
test('malformed candidates, missing contexts and Canvas allocation failures preserve a previous finished surface',async()=>{
 const active=await prepareCompiledSurface(bundle,{createCanvas:()=>canvas()}),snapshot=Uint8ClampedArray.from(active.canvas.painted),bad=structuredClone(bundle);bad.assets[0].base64='AA==';
 await assert.rejects(prepareCompiledSurface(bad,{createCanvas:()=>canvas()}),/digest|size/);
 for(const fault of ['context','allocation']){const failed=canvas(fault);await assert.rejects(prepareCompiledSurface(bundle,{createCanvas:()=>failed}),/unavailable|allocation/);assert.equal(failed.width,0);assert.deepEqual(active.canvas.painted,snapshot);assert.equal(active.canvas.width,32);}
 active.dispose();
});
test('all-instance preparation releases partial candidates on allocation failure or cancellation',async()=>{
 const publication=await createCompiledPanel({id:'prepared',title:'Prepared',viewports:[{pageId:'main',title:'Main',width:320,height:320}]},['a','b'].map((id,i)=>({id,label:id,pageId:'main',bounds:{x:i*.1,y:0,width:.1,height:.1},bundle})));
 const allocated=[];await assert.rejects(prepareCompiledSurfaces(publication,{createCanvas:()=>{const next=canvas(allocated.length?'allocation':undefined);allocated.push(next);return next;}}),/allocation/);assert(allocated.every(c=>c.width===0));
 const controller=new AbortController();controller.abort();await assert.rejects(prepareCompiledSurface(bundle,{signal:controller.signal}),{name:'AbortError'});await assert.rejects(prepareCompiledSurfaces(publication,{signal:controller.signal}),{name:'AbortError'});
 const during=new AbortController(),target=canvas();target.getContext=()=>({putImageData(){during.abort();}});await assert.rejects(prepareCompiledSurface(bundle,{createCanvas:()=>target,signal:during.signal}),{name:'AbortError'});assert.equal(target.width,0);
 const surfaces=await prepareCompiledSurfaces(publication,{createCanvas:()=>canvas()});assert.equal(Object.keys(surfaces).length,2);disposeCompiledSurfaces(surfaces);disposeCompiledSurfaces(undefined);assert(Object.values(surfaces).every(s=>s.canvas.width===0));
});
