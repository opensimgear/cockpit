import test from 'node:test';
import assert from 'node:assert/strict';
import {compileWidget,createToggleSource,createPcmSoundFixture} from '../src/index.ts';
import {ARTIFACT_LIMITS,BROWSER_WIDGET_CAPABILITIES,canonicalWidget,createCompiledPixelDecoder,decodeCompiledFrame,decodeWidgetBase64,encodeWidgetBase64,validateCompiledArtifact,validatePcmWave,validateWidgetCapabilities,verifyCompiledBundle,widgetDigest} from '../../widget-runtime/src/artifact.ts';
import {clone,tinySource,wave} from './fixtures.mjs';

test('strict capability admission rejects invented codecs, limits and media flags',()=>{
  assert(validateWidgetCapabilities(BROWSER_WIDGET_CAPABILITIES).ok);
  for(const change of [c=>c.version=2,c=>c.id='bad:id',c=>c.extra=1,c=>c.rasterFormats=['svg'],c=>c.maxWidth=0,c=>c.maxHeight=3000,c=>c.maxFrames=0,c=>c.maxAssetBytes=100,c=>c.maxTotalBytes=ARTIFACT_LIMITS.totalBytes+1,c=>c.maxResidentBytes=0,c=>c.motion='true',c=>c.audio='false',c=>c.audioFormats=['mp3']]){const c=clone(BROWSER_WIDGET_CAPABILITIES);change(c);assert.equal(validateWidgetCapabilities(c).ok,false);}
});

test('canonical identities and canonical base64 reject ambiguity',async()=>{
  assert.equal(canonicalWidget({z:[false,1,null],a:'x'}),'{"a":"x","z":[false,1,null]}');assert.throws(()=>canonicalWidget({n:Infinity}));
  const bytes=Uint8Array.from([0,1,2,254,255]);assert.deepEqual(decodeWidgetBase64(encodeWidgetBase64(bytes)),bytes);
  for(const value of ['bad','Zh==','!!!!!','eA==\n','x'.repeat(3*1024*1024)])assert.throws(()=>decodeWidgetBase64(value));
  assert.equal(await widgetDigest(new Uint8Array()),'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855');
});

test('artifact validator checks exact schemas, typed selectors, resource and closure contracts',async()=>{
  const bundle=await compileWidget(createToggleSource(),{motion:false,width:64,height:40});assert(validateCompiledArtifact(bundle.artifact).ok);
  const rejects=change=>{const a=clone(bundle.artifact);change(a);const result=validateCompiledArtifact(a);assert.equal(result.ok,false);return result.error;};
  for(const change of [a=>a.extra=1,a=>a.format='old',a=>a.compiler='unknown',a=>a.rasterizer='canvas',a=>a.sourceHash='bad',a=>a.width=0,a=>a.height=3000,a=>a.tileSize=32,a=>a.pixelFormat='rgba',a=>a.behavior.version=0,a=>a.inputs[0].type='enum',a=>a.inputs[0].id='missing-output',a=>a.inputs.push(a.inputs[0]),a=>a.outputs[0].signal='missing',a=>a.inputs[0].min=0,a=>a.hitRegions[0].width=100,a=>a.hitRegions[0].events.activate='missing',a=>a.hitRegions.push(a.hitRegions[0]),a=>a.domains[0].signal='missing',a=>a.domains[0].values=[],a=>a.domains[0].values=[false,0],a=>a.domains[0].numeric=true,a=>a.domains[0].smoothingMs=1,a=>a.domains.push(a.domains[0]),a=>a.domains[0].maxErrorPixels=-1,a=>a.assets.push(a.assets[0]),a=>a.assets[0].mime='image/svg+xml',a=>a.assets[0].sizeBytes=0,a=>a.tiles=[],a=>a.tiles[0].offset=1,a=>a.tiles[0].length=1,a=>a.tiles[0].assetId='missing',a=>a.frames=[],a=>a.frames.push(a.frames[0]),a=>a.frames[0].tiles=[],a=>a.frames[0].tiles[0]=999999,a=>a.frames[0].selector.values.requested='bad',a=>a.frames[0].selector.regions.interaction='missing',a=>a.frames.splice(0,1),a=>a.frames[0].selector=a.frames[1].selector,a=>a.fallbacks=[{feature:'layers',reason:'x'}],a=>a.resources.uniqueTiles=0,a=>a.resources.manifestBytes=0,a=>a.resources.activeCandidateBytes=1])rejects(change);
  const unused={id:'audio-'+'0'.repeat(32),sha256:'0'.repeat(64),mime:'audio/wav',sizeBytes:44};assert.match(rejects(a=>a.assets.push(unused)),/unused/);
  rejects(a=>{const domain=a.domains.find(domain=>domain.signal==='requested');domain.values=[false];a.frames=a.frames.filter(frame=>frame.selector.values.requested===false);});
  rejects(a=>{const domain=a.domains.find(domain=>domain.signal==='quality');domain.values=['fresh'];a.frames=a.frames.filter(frame=>frame.selector.values.quality==='fresh');});
  assert.equal(validateCompiledArtifact(bundle.artifact,{...BROWSER_WIDGET_CAPABILITIES,maxResidentBytes:768}).ok,false);
  const numeric=tinySource();numeric.behavior.signals=[{id:'n',source:'feedback',type:'number',initial:0,min:0,max:1}];numeric.behavior.events=[{id:'feedback',updates:['n']}];numeric.scene[0].width={op:'mul',left:{ref:'n'},right:{literal:32}};numeric.visualDomains=[{signal:'n',values:[0,.5,1],travelPixels:32,maxErrorPixels:8}];numeric.outputs=[{id:'n',label:'Value',type:'number',signal:'n',min:0,max:1}];const n=(await compileWidget(numeric)).artifact;assert(validateCompiledArtifact(n).ok);const bad=clone(n);bad.domains[0].values.reverse();assert.equal(validateCompiledArtifact(bad).ok,false);const port=clone(n);port.outputs[0].min=-1;assert.equal(validateCompiledArtifact(port).ok,false);
  for(const values of [[0],[0,.5],[.5,1]]){const partial=clone(n);partial.domains[0].values=values;partial.frames=partial.frames.filter(frame=>values.includes(frame.selector.values.n));assert.equal(validateCompiledArtifact(partial).ok,false);}
});

test('bundle verification checks every digest and complete payload closure before decode',async()=>{
  const original=await compileWidget(tinySource());assert.deepEqual(await verifyCompiledBundle(original),original);
  const rehash=async bundle=>{bundle.artifactHash=await widgetDigest(new TextEncoder().encode(canonicalWidget(bundle.artifact)));return bundle;};
  for(const change of [b=>b.artifactHash='0'.repeat(64),b=>b.assets=[],b=>b.assets.push(b.assets[0]),b=>b.assets[0].assetId='missing',b=>b.assets[0].base64='AA==',b=>b.assets[0].base64='!']){const b=clone(original);change(b);await assert.rejects(verifyCompiledBundle(b));}
  const behavior=clone(original);behavior.artifact.behaviorHash='0'.repeat(64);await assert.rejects(verifyCompiledBundle(await rehash(behavior)),/behavior digest/);
  const tile=clone(original);tile.artifact.tiles[0].sha256='0'.repeat(64);await assert.rejects(verifyCompiledBundle(await rehash(tile)),/tile digest/);
  const payload=clone(original),bytes=decodeWidgetBase64(payload.assets[0].base64);bytes[0]^=255;payload.assets[0].base64=encodeWidgetBase64(bytes);await assert.rejects(verifyCompiledBundle(payload),/digest/);
  const decoder=createCompiledPixelDecoder(original.artifact,original.assets),pixels=decoder.decode(original.artifact.frames[0].id);assert.equal(pixels.length,32*32*4);assert(pixels instanceof Uint8ClampedArray);assert.equal(decoder.decode(original.artifact.frames[0].id),pixels);assert.throws(()=>decoder.decode('missing'));decoder.dispose();assert.throws(()=>decoder.decode(original.artifact.frames[0].id),/disposed/);
  assert.throws(()=>decodeCompiledFrame(original.artifact,[] ,original.artifact.frames[0].id),/dictionary/);
  const noTile=clone(original.artifact);noTile.frames[0].tiles[0]=999;assert.throws(()=>decodeCompiledFrame(noTile,original.assets,noTile.frames[0].id),/tile/);
});

test('motion and sound admission rejects unsupported or malformed routes/cues',async()=>{
  const source=createToggleSource(),asset=createPcmSoundFixture();source.assets=[asset];source.sounds=[{id:'click',cue:'click',assetId:asset.id}];const original=await compileWidget(source,{width:48,height:32});assert(validateCompiledArtifact(original.artifact).ok);
  const resources=original.artifact.resources,audio=original.artifact.assets.find(asset=>asset.mime==='audio/wav');assert.equal(resources.audioWorkingBytes,audio.sizeBytes*16);assert.equal(resources.residentBytes,resources.transferBytes*3+resources.dictionaryBytes+resources.manifestBytes*4+resources.frameMapBytes*2+48*32*8+768+resources.audioWorkingBytes);assert.equal(resources.activeCandidateBytes,resources.residentBytes*2);const understated=clone(original.artifact);understated.resources.audioWorkingBytes=0;assert.equal(validateCompiledArtifact(understated).ok,false);
  const audioUntouched=original.assets.map(payload=>payload.assetId===audio.id?{assetId:payload.assetId,get base64(){throw new Error('Pixel decoder must never read or retain audio payloads');}}:payload);
  const pixels=createCompiledPixelDecoder(original.artifact,audioUntouched),frameId=original.artifact.frames[0].id;assert.deepEqual(pixels.decode(frameId),decodeCompiledFrame(original.artifact,original.assets,frameId));pixels.dispose();
  for(const change of [a=>a.motions.push(a.motions[0]),a=>a.motions[0].toFrameId='missing',a=>a.motions[0].durationMs=0,a=>a.motions[0].frames=[],a=>a.motions[0].frames[0]=a.motions[0].toFrameId,a=>a.motions[0].interruption='queue',a=>a.sounds.push(a.sounds[0]),a=>a.sounds[0].assetId='missing',a=>a.sounds[0].volume=2,a=>a.sounds[0].stopOnExit='yes',a=>a.sounds[0].cooldownMs=-1]){const a=clone(original.artifact);change(a);assert.equal(validateCompiledArtifact(a).ok,false);}
  assert.equal(validateCompiledArtifact(original.artifact,{...BROWSER_WIDGET_CAPABILITIES,motion:false}).ok,false);assert.equal(validateCompiledArtifact(original.artifact,{...BROWSER_WIDGET_CAPABILITIES,audio:false,audioFormats:[]}).ok,false);
});

test('WAV parser bounds container, PCM format, chunk lengths and duration',()=>{
  assert.deepEqual(validatePcmWave(wave()),{channels:1,sampleRate:8000,durationMs:.25});
  for(const mutate of [b=>b[0]=0,b=>new DataView(b.buffer).setUint32(4,1,true),b=>new DataView(b.buffer).setUint16(20,3,true),b=>new DataView(b.buffer).setUint16(22,3,true),b=>new DataView(b.buffer).setUint32(24,7999,true),b=>new DataView(b.buffer).setUint16(34,8,true),b=>new DataView(b.buffer).setUint32(40,100,true),b=>new DataView(b.buffer).setUint32(40,0,true),b=>new DataView(b.buffer).setUint32(28,1,true)]){const bytes=wave();mutate(bytes);assert.throws(()=>validatePcmWave(bytes));}
  assert.throws(()=>validatePcmWave(new Uint8Array(1)));
  const long=new Uint8Array(44+8000*2*11),template=wave();long.set(template.subarray(0,44));const view=new DataView(long.buffer);view.setUint32(4,long.length-8,true);view.setUint32(40,long.length-44,true);assert.throws(()=>validatePcmWave(long),/ten seconds/);
});
