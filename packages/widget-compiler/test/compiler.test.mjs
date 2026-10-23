import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import {
  BROWSER_WIDGET_CAPABILITIES, clearWidgetCompileCache, compileWidget,
  createLinearGaugeFixture, createPcmSoundFixture, createRadialGaugeFixture,
  createThreeStateFixture, createToggleSource, createWidgetCompileCoordinator,
  decodeCompiledFrame, encodePixelTile, preflightWidget, sampleMotionTrack,
  sourceToSvg, validateWidgetSource, verifyCompiledBundle, widgetDigest,
} from '../src/index.ts';
import { clone, tinySource } from './fixtures.mjs';

test('Toggle seed is the sole product pilot; new generalization fixtures validate',()=>{
  const sources=[createToggleSource(),createLinearGaugeFixture(),createRadialGaugeFixture(),createThreeStateFixture()];
  for(const source of sources)assert.equal(validateWidgetSource(source).ok,true,source.title);
  assert.equal(sources[0].id,'toggle-v2');assert.equal(sources[0].title,'Toggle');
  assert.equal(validateWidgetSource({formatVersion:1,layers:[]}).ok,false);
  assert.equal(preflightWidget(sources[0],{motion:false}).baseFrames,64);
  assert.equal(preflightWidget(sources[1]).domains[0].maxErrorPixels,.9375);
  assert(preflightWidget(sources[2]).domains[0].maxErrorPixels<2);
});

test('pinned real rasterizer produces golden Toggle pixels at declared sizes',async()=>{
  const golden=new Map([['120x72','601c2c0ed0ba4406c9c2332a2574f909c1470abd86a52fedfc6a78ca1557d07c'],['240x144','4870fde96e90ebccd6d60667ec25289c8332b150b5d68ea8085a83a0d2263d0a']]);
  for(const [key,expected] of golden){const [width,height]=key.split('x').map(Number),bundle=await compileWidget(createToggleSource(),{width,height,motion:false});
    const frame=bundle.artifact.frames.find(frame=>{const v=frame.selector.values;return !v.requested&&!v.confirmed&&v.quality==='unknown'&&!v.pressed&&v.available;});
    assert.equal(await widgetDigest(decodeCompiledFrame(bundle.artifact,bundle.assets,frame.id)),expected);
    assert(!JSON.stringify(bundle).includes('fontSize'));assert(!JSON.stringify(bundle).includes('<svg'));assert(!JSON.stringify(bundle).includes('scene'));
  }
});

test('deterministic rebuild, cache identities and parameter changes',async()=>{
  const source=tinySource();source.parameters=[{id:'color',label:'Color',type:'color',default:'#ff0000'}];source.scene[0].fill={ref:'color'};
  clearWidgetCompileCache();const first=await compileWidget(source);clearWidgetCompileCache();const second=await compileWidget(source);assert.deepEqual(second,first);
  const progress=[];const cached=await compileWidget(source,{onProgress:p=>progress.push(p)});assert.equal(cached,second);assert.equal(progress[0].phase,'cached');
  const changed=await compileWidget(source,{parameters:{color:'#00ff00'}});assert.notEqual(changed.artifactHash,first.artifactHash);assert.equal(changed.artifact.behaviorHash,first.artifact.behaviorHash);assert.notEqual(changed.artifact.parameterHash,first.artifact.parameterHash);
  const resized=await compileWidget(source,{width:48});assert.notEqual(resized.artifactHash,first.artifactHash);await verifyCompiledBundle(first);
});

test('RGB565 byte order, straight alpha, edge padding and tile seams',async()=>{
  const tile=encodePixelTile(Uint8Array.from([255,0,0,255,0,255,0,128,255,255,255,0]),3,1,0,0);
  assert.deepEqual([...tile.subarray(0,6)],[0,248,224,7,0,0]);assert.deepEqual([...tile.subarray(512,516)],[255,128,0,0]);assert.equal(tile[767],0);
  const source=tinySource();source.size={width:17,height:17};source.scene=[{id:'solid',kind:'rectangle',x:0,y:0,width:17,height:17,fill:'#ff0000',opacity:.5}];
  const bundle=await compileWidget(source),pixels=decodeCompiledFrame(bundle.artifact,bundle.assets,bundle.artifact.frames[0].id);
  for(const [x,y] of [[0,0],[15,15],[16,0],[0,16],[16,16]])assert.deepEqual([...pixels.subarray((y*17+x)*4,(y*17+x)*4+4)],[255,0,0,128]);
  assert.equal(bundle.artifact.frames[0].tiles.length,4);assert(bundle.artifact.tiles.length<=4);
});

test('nonmatching target ratios keep compiled pixels and hit regions in the same coordinates',async()=>{
  const source=tinySource();source.behavior.events=[{id:'activate'}];source.scene=[{id:'left',kind:'rectangle',x:0,y:0,width:16,height:32,fill:'#ff0000'}];source.hitRegions=[{id:'left',x:0,y:0,width:16,height:32,events:{activate:'activate'}}];
  const bundle=await compileWidget(source,{width:48,height:16}),pixels=decodeCompiledFrame(bundle.artifact,bundle.assets,bundle.artifact.frames[0].id);assert.deepEqual(bundle.artifact.hitRegions[0],{id:'left',x:0,y:0,width:24,height:16,events:{activate:'activate'}});
  for(const [x,alpha] of [[0,255],[23,255],[24,0],[47,0]])assert.equal(pixels[(8*48+x)*4+3],alpha);
});

test('target capability fallbacks remove motion and sound assets while preserving behavior',async()=>{
  const source=createToggleSource(),sound=createPcmSoundFixture();source.assets=[sound];source.sounds=[{id:'click',cue:'click',assetId:sound.id,cooldownMs:20,volume:.2,stopOnExit:true}];
  const full=await compileWidget(source,{width:120,height:72}),capabilities={...BROWSER_WIDGET_CAPABILITIES,id:'silent-static',motion:false,audio:false,audioFormats:[]};
  const fallback=await compileWidget(source,{width:120,height:72,capabilities});assert(full.artifact.motions.length>0);assert.equal(full.artifact.sounds.length,1);assert.equal(full.artifact.assets.filter(asset=>asset.mime==='audio/wav').length,1);
  assert.deepEqual(fallback.artifact.fallbacks.map(f=>f.feature),['motion','audio']);assert.equal(fallback.artifact.motions.length,0);assert.equal(fallback.artifact.sounds.length,0);assert.equal(fallback.artifact.assets.some(asset=>asset.mime==='audio/wav'),false);assert.deepEqual(fallback.artifact.behavior,full.artifact.behavior);
  await verifyCompiledBundle(full);await verifyCompiledBundle(fallback,capabilities);
  const preference=await compileWidget(source,{width:120,height:72,motion:false,audio:false});assert(preference.artifact.fallbacks.every(f=>f.reason.includes('preference')));
});

test('motion fallback disables numeric smoothing while keeping every live value frame',async()=>{
  const source=createLinearGaugeFixture();source.visualDomains[0].smoothingMs=120;
  const dimensions={width:64,height:16},animated=await compileWidget(source,dimensions),disabled=await compileWidget(source,{...dimensions,motion:false}),capabilities={...BROWSER_WIDGET_CAPABILITIES,id:'static-gauge',motion:false},staticTarget=await compileWidget(source,{...dimensions,capabilities});
  assert.equal(animated.artifact.domains[0].smoothingMs,120);assert.equal(disabled.artifact.domains[0].smoothingMs,0);assert.equal(staticTarget.artifact.domains[0].smoothingMs,0);
  assert.deepEqual(staticTarget.artifact.frames,animated.artifact.frames);assert.deepEqual(disabled.artifact.frames,animated.artifact.frames);assert.equal(staticTarget.artifact.fallbacks[0].feature,'motion');assert(staticTarget.artifact.fallbacks[0].reason.includes('live values'));
  assert.notDeepEqual(decodeCompiledFrame(staticTarget.artifact,staticTarget.assets,staticTarget.artifact.frames[0].id),decodeCompiledFrame(staticTarget.artifact,staticTarget.assets,staticTarget.artifact.frames.at(-1).id));await verifyCompiledBundle(staticTarget,capabilities);
});

test('generic region transitions bake property and transform tracks into finished pixels',async()=>{
  const source=tinySource();source.behavior.regions=[{id:'mode',initial:'off',states:['off','on'],transitions:[]}];source.scene=[{id:'dot',kind:'ellipse',x:0,y:8,width:8,height:8,fill:'#00ff00',transform:{translateX:{op:'if',condition:{op:'eq',left:{state:'mode'},right:{literal:'on'}},then:{literal:24},else:{literal:0}}}}];
  source.motions=[{id:'move',from:{},to:{},fromRegions:{mode:'off'},toRegions:{mode:'on'},durationMs:100,fps:30,tracks:[{node:'dot',property:'transform.translateX',keyframes:[{at:0,value:0},{at:1,value:24}],easing:'ease-out'}]}];
  const bundle=await compileWidget(source);assert.equal(bundle.artifact.motions.length,1);const motion=bundle.artifact.motions[0];assert.equal(motion.frames.length,4);assert.equal(motion.interruption,'snap');
  const first=decodeCompiledFrame(bundle.artifact,bundle.assets,motion.frames[0]),middle=decodeCompiledFrame(bundle.artifact,bundle.assets,motion.frames[1]),last=decodeCompiledFrame(bundle.artifact,bundle.assets,motion.frames.at(-1));assert.notDeepEqual(middle,first);assert.notDeepEqual(middle,last);
  for(const easing of ['linear','ease-in','ease-out','ease-in-out']){const track={node:'dot',property:'x',keyframes:[{at:0,value:0},{at:1,value:100}],easing};assert.equal(sampleMotionTrack(track,0),0);assert.equal(sampleMotionTrack(track,1),100);assert(sampleMotionTrack(track,.25)>0);assert(sampleMotionTrack(track,.75)<100);}
});

test('generic primitives, parameters and image artwork use the authoring-only SVG boundary',async()=>{
  const source=tinySource();source.parameters=[{id:'caption',label:'Caption',type:'text',default:'A<&"\''},{id:'position',label:'Position',type:'number',default:1,min:0,max:4},{id:'visible',label:'Visible',type:'boolean',default:true}];
  source.scene=[{id:'group',kind:'group',opacity:.75,transform:{translateX:1,translateY:1,rotate:0,scaleX:1,scaleY:1,originX:0,originY:0},children:[{id:'hidden',kind:'rectangle',x:0,y:0,width:1,height:1,visible:false},{id:'path',kind:'path',d:'M 0 0 L 10 0 L 10 10 Z',fill:'#123456'},{id:'line',kind:'line',x:0,y:16,x2:32,y2:16,stroke:'#ffffff',strokeWidth:1},{id:'caption',kind:'text',x:{ref:'position'},y:24,text:{ref:'caption'},font:'sans',fontSize:8,align:'left',fill:'#ffffff',visible:{ref:'visible'}}]}];
  const svg=sourceToSvg(source,{values:{},regions:{},parameters:{caption:'A<&"\'',position:1,visible:true}});assert(svg.includes('&lt;'));assert(svg.includes('&amp;'));assert(svg.includes('&quot;'));assert(svg.includes('&apos;'));assert(!svg.includes('id="hidden"'));
  await compileWidget(source);source.scene.push({id:'mono',kind:'text',x:30,y:30,text:'Hi',font:'mono',fontSize:8,align:'right',fill:'#ffffff'});await compileWidget(source);
});

test('feasibility preflight rejects unbounded, inaccurate and combinatorial presentations',async()=>{
  const source=createLinearGaugeFixture();source.visualDomains=[];assert.throws(()=>preflightWidget(source),/Unbounded/);
  const inaccurate=createLinearGaugeFixture();inaccurate.visualDomains[0].samples=2;assert.throws(()=>preflightWidget(inaccurate),/pixel error/);
  const exploding=tinySource();for(let i=0;i<8;i++)exploding.behavior.regions.push({id:'region'+i,initial:'a',states:['a','b','c','d'],transitions:[]});assert.throws(()=>preflightWidget(exploding),/combination/);
  assert.throws(()=>preflightWidget(createToggleSource(),{capabilities:{...BROWSER_WIDGET_CAPABILITIES,maxFrames:3}}),/frame count/);
  const overBudget=tinySource();overBudget.scene.push({id:'second-color',kind:'rectangle',x:0,y:0,width:16,height:32,fill:'#00ff00'});await assert.rejects(compileWidget(overBudget,{capabilities:{...BROWSER_WIDGET_CAPABILITIES,maxTotalBytes:768}}),/byte budget/);
  await assert.rejects(compileWidget(tinySource(),{capabilities:{...BROWSER_WIDGET_CAPABILITIES,maxResidentBytes:768}}),/resident memory/);
  const finite=createLinearGaugeFixture();finite.visualDomains=[{signal:'rpm',values:Array.from({length:129},(_,i)=>i*62.5),travelPixels:240,maxErrorPixels:1}];assert.equal(preflightWidget(finite).baseFrames,1032);
  const huge=tinySource();huge.size={width:2048,height:2048};huge.behavior.regions=[{id:'big',initial:'s0',states:Array.from({length:32},(_,i)=>'s'+i),transitions:[]},{id:'other',initial:'a',states:['a','b','c','d'],transitions:[]}];assert.throws(()=>preflightWidget(huge),/work budget/);
});

test('cancellation and editor revisions fence asynchronous results',async()=>{
  const aborted=new AbortController();aborted.abort();await assert.rejects(compileWidget(tinySource(),{signal:aborted.signal}),{name:'AbortError'});
  clearWidgetCompileCache();const during=new AbortController();await assert.rejects(compileWidget(tinySource(),{signal:during.signal,onProgress(){during.abort();}}),{name:'AbortError'});
  const coordinator=createWidgetCompileCoordinator();assert.equal(coordinator.current,undefined);const first=coordinator.compile(createToggleSource(),{motion:false,width:119,height:71});coordinator.invalidate();await assert.rejects(first,{name:'AbortError'});assert.equal(coordinator.current,undefined);
  const second=await coordinator.compile(tinySource());assert.equal(coordinator.current,second);assert(coordinator.revision>=3);
  const external=new AbortController();external.abort();await assert.rejects(coordinator.compile(tinySource(),{signal:external.signal}),{name:'AbortError'});
  const oldSignal=new AbortController(),old=coordinator.compile(createToggleSource(),{motion:false,width:118,height:70,signal:oldSignal.signal}),next=coordinator.compile(tinySource(),{width:33,height:33});oldSignal.abort();await assert.rejects(old,{name:'AbortError'});assert.equal((await next).artifact.width,33);
});

test('font bank hash is a build identity invariant',()=>{
  const path=new URL('../src/index.ts',import.meta.url).href;const code=`import {initializeWidgetCompiler} from ${JSON.stringify(path)};try{await initializeWidgetCompiler({fonts:[new Uint8Array([1]),new Uint8Array([2])]});process.exitCode=1;}catch(e){if(!String(e).includes('font hash'))throw e;}`;
  const result=spawnSync(process.execPath,['--input-type=module','-e',code],{encoding:'utf8'});assert.equal(result.status,0,result.stderr);
});
