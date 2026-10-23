import {behaviorToggleSource as createToggleSource} from './behavior-toggle.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import {  createLinearGaugeFixture, createThreeStateFixture, createPcmSoundFixture, compileWidget, expressionRefs, imageDimensions, resolvedParameters, resolveVisualValue, sourceToSvg, validateWidgetSource } from '../src/index.ts';
import { clone,tinySource,rasterPng } from './fixtures.mjs';
function rejected(change,base=createToggleSource()){const input=clone(base);change(input);const result=validateWidgetSource(input);assert.equal(result.ok,false,JSON.stringify(input));return result.error;}

test('strict source schema rejects malformed names, budgets, ports, bounds and behavior',()=>{
  assert.equal(validateWidgetSource(null).ok,false);
  for(const change of [s=>s.extra=true,s=>s.formatVersion=1,s=>s.id='bad:id',s=>s.revision=0,s=>s.title='',s=>s.size.width=0,s=>s.parameters.push(s.parameters[0]),s=>s.parameters[0].type='script',s=>s.parameters[0].default='\n',s=>s.parameters[1].default='red',s=>s.behavior.version=7,s=>s.inputs.push(s.inputs[0]),s=>s.inputs[0].signal='missing',s=>s.inputs[0].type='number',s=>s.inputs[0].id='missing-emit',s=>s.hitRegions[0].width=999,s=>s.hitRegions[0].events.activate='missing',s=>s.hitRegions.push(s.hitRegions[0]),s=>s.scene=[],s=>s.scene.push(s.scene[0]),s=>s.scene[0].kind='shader',s=>s.scene[0].font='system',s=>s.scene[0].align='arbitrary',s=>s.scene[0].bogus=1,s=>s.scene[0].text={ref:'missing'},s=>s.scene[0].text={state:'missing'},s=>s.scene[0].visible={op:'evil',value:true},s=>s.scene[0].transform={exec:1},s=>s.assets=[{id:'bad',mime:'text/html',base64:'eA=='}],s=>s.assets=[{id:'bad',mime:'image/png',base64:'!'}],s=>s.assets=[{id:'sound',mime:'audio/wav',base64:'eA=='}]])rejected(change);
  assert.match(rejected(s=>s.title='x'.repeat(2*1024*1024)),/byte budget/);
  const numeric=createLinearGaugeFixture();rejected(s=>s.outputs[0].min=9000,numeric);rejected(s=>s.outputs[0].max=-1,numeric);rejected(s=>s.outputs[0].min=1,numeric);rejected(s=>s.outputs[0].max=7999,numeric);rejected(s=>s.outputs[0].values=[],numeric);rejected(s=>s.inputs[0].min=0);
  const three=createThreeStateFixture();rejected(s=>s.inputs[0].values=['low'],three);rejected(s=>s.behavior.outputs[0].neutral='high',three);
});

test('visual expression bounds match evaluation independently of scene group nesting',async()=>{
  const source=tinySource();source.behavior.signals=[{id:'enabled',source:'local',type:'boolean',initial:true}];
  let visible={ref:'enabled'};for(let i=0;i<7;i++)visible={op:'not',value:visible};source.scene[0].visible=visible;
  let scene=source.scene;for(let i=0;i<8;i++)scene=[{id:'group'+i,kind:'group',children:scene}];source.scene=scene;
  assert(validateWidgetSource(source).ok);const compiled=await compileWidget(source);assert.deepEqual(compiled.artifact.domains[0].values,[false,true]);assert.equal(compiled.artifact.frames.length,2);
  rejected(s=>s.scene[0].visible={op:'not',value:visible},source);
  const maximal={op:'and',values:Array.from({length:16},(_,i)=>i===15?{literal:true}:{op:'not',value:{literal:false}})};
  const bounded=tinySource();bounded.scene[0].visible=maximal;assert(validateWidgetSource(bounded).ok);await compileWidget(bounded);
  rejected(s=>s.scene[0].visible={op:'and',values:Array.from({length:16},()=>({op:'not',value:{literal:false}}))},bounded);
  rejected(s=>s.scene[0].visible={op:'not',value:true},bounded);
  rejected(s=>s.scene[0].visible={literal:'x'.repeat(121)},bounded);
});

test('typed parameters and bounded expressions stay data, with no executable hooks',()=>{
  const source=tinySource();source.parameters=[{id:'bool',label:'Boolean',type:'boolean',default:true},{id:'number',label:'Number',type:'number',default:1,min:0,max:2},{id:'text',label:'Text',type:'text',default:'sample'}];assert(validateWidgetSource(source).ok);
  assert.deepEqual(resolvedParameters(source,{bool:false,number:2}),{bool:false,number:2,text:'sample'});
  for(const overrides of [{bool:'yes'},{number:3},{text:2},{unknown:2}])assert.throws(()=>resolvedParameters(source,overrides));
  rejected(s=>s.parameters[0].default='true',source);rejected(s=>s.parameters[1].min=3,source);rejected(s=>s.parameters[0].min=0,source);
  const expressions=[{literal:true},{op:'not',value:{literal:false}},{op:'and',values:[{literal:true},{literal:true}]},{op:'or',values:[{literal:false},{literal:true}]},{op:'lt',left:{literal:1},right:{literal:2}},{op:'clamp',value:{literal:2},min:{literal:0},max:{literal:1}},{op:'if',condition:{literal:true},then:{literal:1},else:{literal:2}}];
  for(const expression of expressions){source.scene[0].opacity=typeof resolveVisualValue(expression,{values:{},regions:{}},0)==='boolean'?{op:'if',condition:expression,then:{literal:1},else:{literal:0}}:expression;assert(validateWidgetSource(source).ok);}
  const refs=expressionRefs({value:{ref:'number'},child:{state:'mode'},many:[{ref:'text'}]});assert.deepEqual([...refs.refs],['number','text']);assert.deepEqual([...refs.regions],['mode']);
  assert.equal(resolveVisualValue(undefined,{values:{},regions:{}},'fallback'),'fallback');
  let deep={literal:true};for(let i=0;i<20;i++)deep={op:'not',value:deep};rejected(s=>s.scene[0].visible=deep,source);
  rejected(s=>s.scene[0].visible={op:'and',values:[]},source);rejected(s=>s.scene[0].visible={literal:null},source);
});

test('finite domains require full coverage and explicit continuous accuracy',()=>{
  const source=createToggleSource();source.visualDomains=[{signal:'requested',values:[false,true]}];assert(validateWidgetSource(source).ok);
  for(const domain of [{signal:'requested',values:[false]},{signal:'requested',values:[false,true,true]},{signal:'requested',values:[0,1]},{signal:'quality',values:['fresh']},{signal:'quality',values:['fresh','unknown','stale','evil']},{signal:'requested',min:0,max:1,samples:2},{signal:'missing',values:[true]},{signal:'requested',values:[true,false],smoothingMs:10}])rejected(s=>s.visualDomains=[domain]);
  const numeric=createLinearGaugeFixture();for(const domain of [{signal:'rpm',min:0,max:9000,samples:3,maxErrorPixels:1,travelPixels:10},{signal:'rpm',min:0,max:8000,samples:1,maxErrorPixels:1,travelPixels:10},{signal:'rpm',values:[0,8000]},{signal:'rpm',values:[0,8000],min:0,maxErrorPixels:1,travelPixels:10},{signal:'rpm',values:[0,4000],maxErrorPixels:1,travelPixels:10}])rejected(s=>s.visualDomains=[domain],numeric);
  numeric.visualDomains[0].smoothingMs=100;assert(validateWidgetSource(numeric).ok);rejected(s=>s.visualDomains.push(s.visualDomains[0]),numeric);
});

test('bounded motion and cue source contracts reject invalid routes/tracks/media',()=>{
  for(const change of [s=>s.motions.push(s.motions[0]),s=>s.motions[0].durationMs=0,s=>s.motions[0].durationMs=60000,s=>s.motions[0].interruption='queue',s=>s.motions[0].from={},s=>s.motions[0].from={missing:true},s=>s.motions[0].from={requested:2},s=>s.motions[0].fromRegions={missing:'ready'},s=>s.motions[0].fromRegions={interaction:'missing'},s=>s.motions[0].tracks[0].node='missing',s=>s.motions[0].tracks[0].property='draw',s=>s.motions[0].tracks[0].easing='spring',s=>s.motions[0].tracks[0].keyframes=[{at:0,value:1}],s=>s.motions[0].tracks[0].keyframes[0].at=.5,s=>s.motions[0].tracks[0].keyframes[1].at=.5])rejected(change);
  const source=createToggleSource(),asset=createPcmSoundFixture();source.assets=[asset];source.sounds=[{id:'click',cue:'click',assetId:asset.id}];assert(validateWidgetSource(source).ok);
  for(const change of [s=>s.sounds.push(s.sounds[0]),s=>s.sounds[0].assetId='missing',s=>s.sounds[0].cooldownMs=-1,s=>s.sounds[0].volume=2,s=>s.sounds[0].stopOnExit='yes',s=>s.assets.push(s.assets[0])])rejected(change,source);
});

test('shape rendering resolves safe paints, opacity, paths, dimensions and transforms',async()=>{
  const source=tinySource();for(const change of [s=>s.scene[0].fill='url(https://bad)',s=>s.scene[0].opacity=2,s=>s.scene[0].visible=1,s=>s.scene[0].width=-1,s=>s.scene=[{id:'path',kind:'path',d:'<script>'}],s=>s.scene=[{id:'text',kind:'text',x:0,y:0,text:'x',font:'sans',fontSize:0}]]){const s=clone(source);change(s);await assert.rejects(compileWidget(s));}
  const context={values:{},regions:{}};source.scene[0].fill='transparent';assert(sourceToSvg(source,context).includes('transparent'));
  source.scene[0].transform={translateX:{op:'sub',left:{literal:2},right:{literal:1}},translateY:0,rotate:10,scaleX:.5,scaleY:2,originX:16,originY:16};assert(sourceToSvg(source,context).includes('rotate(10)'));
  let group=source.scene;for(let i=0;i<10;i++)group=[{id:'nested'+i,kind:'group',children:group}];source.scene=group;assert.equal(validateWidgetSource(source).ok,false);
});

test('source raster imports bound decoded dimensions and support explicit codecs',async()=>{
  const png=rasterPng();assert.deepEqual(imageDimensions(png,'image/png'),{width:1,height:1});
  const source=tinySource();source.assets=[{id:'pixel',mime:'image/png',base64:Buffer.from(png).toString('base64')}];source.scene=[{id:'image',kind:'image',x:0,y:0,width:32,height:32,assetId:'pixel'}];assert(validateWidgetSource(source).ok);await compileWidget(source);rejected(s=>s.scene[0].assetId='missing',source);
  const jpeg=Uint8Array.from([255,216,255,192,0,11,8,0,2,0,3,1,1,17,0,255,217]);assert.deepEqual(imageDimensions(jpeg,'image/jpeg'),{width:3,height:2});
  const webp=new Uint8Array(30);webp.set(Buffer.from('RIFF'));webp.set(Buffer.from('WEBP'),8);webp.set(Buffer.from('VP8X'),12);new DataView(webp.buffer).setUint32(4,22,true);new DataView(webp.buffer).setUint32(16,10,true);webp[24]=2;webp[27]=1;assert.deepEqual(imageDimensions(webp,'image/webp'),{width:3,height:2});webp[20]=2;assert.throws(()=>imageDimensions(webp,'image/webp'),/Animated/);
  webp[20]=47;webp.set(Buffer.from('VP8L'),12);webp[21]=2;webp[22]=64;webp[23]=0;webp[24]=0;assert.deepEqual(imageDimensions(webp,'image/webp'),{width:3,height:2});webp.set(Buffer.from('VP8 '),12);webp[23]=157;webp[24]=1;webp[25]=42;webp[26]=3;webp[27]=0;webp[28]=2;assert.deepEqual(imageDimensions(webp,'image/webp'),{width:3,height:2});
  for(const mime of ['image/png','image/jpeg','image/webp','image/svg+xml'])assert.throws(()=>imageDimensions(new Uint8Array(40),mime));
  const huge=Uint8Array.from(png);new DataView(huge.buffer).setUint32(16,8192);assert.throws(()=>imageDimensions(huge,'image/png'));
});
