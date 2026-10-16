import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { EXPR, NONE, OP, PROPERTY, Q, RECORD_BYTES, SECTION, SUNTON_PROFILE, WAVESHARE_PROFILE, canonical, checkProfile, digest, evaluate, expressionKind, expressionRange, expressionStackDepth, formatFixed, inspectBinaryObject, inspectContent, packBinaryObject, packPackage, profileHash, records, section, toFixed, validateManifest, verifyPackage } from '../src/index.ts';
import { fixtureObjects, fixturePackage, record } from './build-fixtures.mjs';

const encoder=new TextEncoder(),fixtureDir=new URL('./fixtures/',import.meta.url);
const source={sourceUiId:'panel',sourcePublicationHash:'sha256:'+'1'.repeat(64),semanticFingerprint:'sha256:'+'2'.repeat(64),compilerId:'compiler',compilerVersion:'1.0.0'};
const memory={internalRamBytes:8192,psramBytes:0,sceneRamBytes:0,snapshotBytes:1024,tileBytes:1024,transferBytes:2048,cacheBytes:0};
function change(objects,kind,sectionId,mutator){
  const next=new Map(objects),object=inspectBinaryObject(kind,next.get(kind));
  const sections=[...object.sections.values()].map(item=>({kind:item.kind,data:Uint8Array.from(item.data)}));
  const target=sections.find(item=>item.kind===sectionId);assert.ok(target);mutator(new DataView(target.data.buffer),target.data,sections);
  next.set(kind,packBinaryObject(kind,sections));return next;
}
function inspect(objects,profile=SUNTON_PROFILE){return inspectContent(new Map([...objects].map(([kind,bytes])=>[kind,inspectBinaryObject(kind,bytes)])),profile);}
async function assertManifestFails(mutator,pattern){const fixture=await fixturePackage(),manifest=structuredClone(fixture.manifest);mutator(manifest);assert.throws(()=>validateManifest(manifest),pattern);}

test('exact binary and canonical manifest golden vectors, both profiles',async()=>{
  const expected=JSON.parse(await readFile(new URL('golden.json',fixtureDir),'utf8'));
  for(const [profile,key]of[[SUNTON_PROFILE,''],[WAVESHARE_PROFILE,'waveshare']]){
    const fixture=await fixturePackage(profile),prefix=key?`${key}P`:'p';
    assert.equal(fixture.packageHash,expected[`${prefix}ackageHash`]);assert.equal(await profileHash(profile),expected[key?'waveshareProfileHash':'profileHash']);
    assert.equal(canonical(profile),await readFile(new URL(key?'waveshare-profile.json':'sunton-profile.json',fixtureDir),'utf8'));
    assert.deepEqual(fixture.manifestBytes,new Uint8Array(await readFile(new URL(key?'waveshare-manifest.json':'manifest.json',fixtureDir))));
    for(const [kind,bytes]of fixture.objects)assert.deepEqual(bytes,new Uint8Array(await readFile(new URL(key&&kind==='scene'?'waveshare-scene.bin':`${kind}.bin`,fixtureDir))));
    const verified=await verifyPackage(fixture.manifestBytes,fixture.objects,profile,fixture.packageHash,{internalRamBytes:8192,psramBytes:0});
    assert.deepEqual(verified.manifest,fixture.manifest);assert.notEqual(verified.objects.get('scene'),fixture.objects.get('scene'));
  }
});
test('ABI table golden agrees and arrays are immutable profile definitions',async()=>{
  assert.deepEqual(JSON.parse(await readFile(new URL('abi.json',fixtureDir),'utf8')).recordBytes,RECORD_BYTES);
  assert.ok(Object.isFrozen(SUNTON_PROFILE.limits));assert.ok(Object.isFrozen(SUNTON_PROFILE.features));
  assert.equal(SUNTON_PROFILE.features.includes('input.xy'),false);assert.equal(WAVESHARE_PROFILE.features.includes('input.barycentric3'),false);
});
test('integer canonical form is deterministic and rejects non-wire values',async()=>{
  assert.equal(canonical({z:[true,null,1],a:{y:'x',a:0}}),'{"a":{"a":0,"y":"x"},"z":[true,null,1]}');
  for(const invalid of[undefined,NaN,Infinity,1.25,2n,new Date(),new Map(),{x:undefined}])assert.throws(()=>canonical(invalid));
  assert.equal(await digest(encoder.encode('abc')),'sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
});
test('binary header rejects malformed versions, directory, offsets, flags and counts',()=>{
  const original=fixtureObjects().get('scene');
  const corruptions=[
    data=>data.setUint8(0,0),data=>data.setUint16(4,2,true),data=>data.setUint16(6,31,true),data=>data.setUint32(8,0,true),
    data=>data.setUint32(12,36,true),data=>data.setUint16(16,0,true),data=>data.setUint16(16,65535,true),data=>data.setUint16(18,1,true),data=>data.setUint8(31,1),
    data=>data.setUint16(32,65535,true),data=>data.setUint16(34,1,true),data=>data.setUint32(36,0,true),data=>data.setUint32(40,0,true),data=>data.setUint32(40,0xffffffff,true),data=>data.setUint32(44,0,true),
    data=>data.setUint16(48,1,true),data=>data.setUint32(52,data.getUint32(36,true),true),data=>data.setUint32(52,data.getUint32(52,true)+1,true),
  ];
  for(const mutation of corruptions){const bytes=Uint8Array.from(original);mutation(new DataView(bytes.buffer));assert.throws(()=>inspectBinaryObject('scene',bytes));}
  assert.throws(()=>inspectBinaryObject('scene',original,32));assert.throws(()=>inspectBinaryObject('scene',new Uint8Array(31)));
  const trailing=new Uint8Array(original.length+1);trailing.set(original);new DataView(trailing.buffer).setUint32(8,trailing.length,true);assert.throws(()=>inspectBinaryObject('scene',trailing),/trailing/);
  const pad=packBinaryObject('images',[{kind:1,data:new Uint8Array(24)},{kind:2,data:new Uint8Array(1)}]);
  assert.equal(inspectBinaryObject('images',pad).sections.size,2);
  for(const invalid of[[],[{kind:99,data:new Uint8Array(1)}],[{kind:1,data:new Uint8Array(1)}],[{kind:1,data:new Uint8Array(24)},{kind:1,data:new Uint8Array(24)}],[{kind:2,data:new Uint8Array(2560*1024)}]])assert.throws(()=>packBinaryObject('images',invalid));
  assert.equal(section(inspectBinaryObject('images',pad),99).count,0);
});
test('golden content derives work from all reachable tile memberships',()=>{
  const facts=inspect(fixtureObjects());assert.deepEqual(facts.counts,{pages:1,nodes:4,variables:1,bindings:1,hitRegions:1,glyphs:1,timelines:0});
  assert.equal(facts.maxDirtyPixels,1536);assert.equal(facts.maxLayersPerTile,3);assert.equal(facts.maxUpdateInstructions,1);
  assert.deepEqual(facts.requires,['input.momentary','scene.image565','scene.mask4','scene.rect','scene.text']);
});
test('semantics validate IDs, target, descriptor and once-only ownership',()=>{
  const objects=fixtureObjects();
  const corruptions=[
    [1,4,'Uint16',0],[1,6,'Uint8',9],[1,7,'Uint8',1],[1,8,'Uint32',0xffffffff],[1,20,'Int32',0],[1,24,'Int32',1],[1,28,'Uint16',NONE],[1,30,'Uint16',0],
    [2,6,'Uint8',9],[2,7,'Uint8',1],[2,14,'Uint16',120],[2,16,'Int32',65537],[2,24,'Uint32',0xffffffff],[2,30,'Uint16',1],
    [3,6,'Uint8',9],[3,7,'Uint8',1],[3,8,'Uint16',NONE],[3,10,'Uint16',0],[3,14,'Uint16',0],[3,16,'Uint16',1],[3,20,'Uint32',1],
  ];
  for(const [table,offset,type,value]of corruptions)assert.throws(()=>inspect(change(objects,'semantics',table,data=>data[`set${type}`](offset,value,true))));
  assert.throws(()=>inspect(change(objects,'semantics',4,(data)=>data.setUint8(0,0))));
  assert.throws(()=>inspect(new Map([['scene',objects.get('scene')]])),/Missing/);
});
test('scene rejects invalid references, unsafe bounds, geometry and spatial data',()=>{
  const objects=fixtureObjects();
  const corruptions=[
    [1,6,'Uint16',1],[1,8,'Uint16',5],[1,12,'Uint32',1],
    [2,0,'Uint16',1],[2,2,'Uint8',255],[2,3,'Uint8',2],[2,4,'Int16',-1],[2,12,'Int16',-1],[2,20,'Int16',9],[2,28,'Uint16',0],[2,30,'Uint16',1],[2,32,'Uint16',0],[2,34,'Uint16',1],[2,42,'Uint16',1],[2,44,'Uint32',1],[2,52,'Uint16',1],[2,56,'Uint32',1],
    [2,64+28,'Uint16',1],[2,64+52,'Uint16',2],[2,128+30,'Uint16',0],[2,192+32,'Uint16',NONE],[2,192+50,'Uint16',121],
    [3,0,'Uint16',NONE],[3,2,'Uint8',2],[3,3,'Uint8',5],[3,4,'Int32',-1],[3,4,'Int32',32768],[3,12,'Int32',0],[3,18,'Uint16',1],
    [4,0,'Uint16',NONE],[4,2,'Uint8',255],[4,3,'Uint8',1],[4,4,'Uint16',1],[4,6,'Uint16',0],[4,8,'Int32',1],[4,12,'Int32',65537],
    [5,0,'Uint8',255],[5,1,'Uint8',1],[5,2,'Uint16',1],[5,4,'Int32',1],[5,8,'Int32',1],
    [8,0,'Uint16',NONE],[8,2,'Uint16',NONE],[8,4,'Int16',0],[8,12,'Uint16',NONE],[8,14,'Uint16',1],
    [11,0,'Uint16',1],[11,2,'Uint16',65535],[11,6,'Uint16',1],[11,8,'Uint16',0],[11,10,'Uint16',1],[12,0,'Uint16',NONE],
  ];
  for(const [table,offset,type,value]of corruptions)assert.throws(()=>inspect(change(objects,'scene',table,data=>data[`set${type}`](offset,value,true))),`table ${table} offset ${offset}`);
});
test('image and font records are decoded-bounds checked including A4 padding',()=>{
  const objects=fixtureObjects();
  for(const [kind,table,offset,type,value]of[
    ['images',1,0,'Uint16',0],['images',1,2,'Uint16',481],['images',1,4,'Uint32',3],['images',1,8,'Uint32',0xffffffff],['images',1,12,'Uint32',7],['images',1,16,'Uint8',9],['images',1,17,'Uint8',1],
    ['fonts',1,2,'Uint16',0],['fonts',1,4,'Uint16',1],['fonts',1,6,'Uint16',0],['fonts',1,10,'Uint16',1],
    ['fonts',2,0,'Uint32',0xd800],['fonts',2,4,'Uint16',257],['fonts',2,8,'Int16',-1],['fonts',2,12,'Int16',1],['fonts',2,12,'Int16',-1],['fonts',2,14,'Uint16',1],['fonts',2,16,'Uint32',0xffffffff],['fonts',2,20,'Uint32',1],
  ])assert.throws(()=>inspect(change(objects,kind,table,data=>data[`set${type}`](offset,value,true))));
  let odd=change(objects,'images',1,data=>data.setUint16(24,1,true));assert.throws(()=>inspect(odd),/padding/);
  odd=change(objects,'fonts',2,data=>data.setUint16(4,1,true));assert.throws(()=>inspect(odd),/padding/);
  const minimal=new Map(objects);minimal.delete('images');assert.throws(()=>inspect(minimal),/image/);minimal.set('images',objects.get('images'));minimal.delete('fonts');assert.throws(()=>inspect(minimal),/font/);
});
test('manifest exact schema and canonical encoding reject unsupported data',async()=>{
  for(const [mutation,pattern]of[
    [m=>{m.extra=true;},/fields/],[m=>{m.formatVersion=2;},/format/],[m=>{m.rendererAbi=2;},/format/],[m=>{m.sourceUiId=' bad';},/identifier/],
    [m=>{m.sourcePublicationHash='sha256:bad';},/digest/],[m=>{m.viewport.width=0;},/viewport/],[m=>{m.viewport.content=[0,0,801,1];},/bounds/],[m=>{m.viewport.content=[0,0,480,321];},/outside/],
    [m=>{m.requires.push(m.requires[0]);},/feature/],[m=>{m.requires=['scene.bad feature'];},/feature/],[m=>{m.counts.nodes=-1;},/bounds/],[m=>{m.resources.tileBytes=1.5;},/bounds/],
    [m=>{m.objects.reverse();},/object/],[m=>{m.objects[0].version=2;},/object/],[m=>{m.objects[0].bytes=1;},/Truncated/],[m=>{m.objects[0].sha256='bad';},/digest/],
    [m=>{m.semanticDescriptorHash='sha256:'+'0'.repeat(64);},/semantic/],[m=>{m.resources.objectBytes++;},/accounting/],
  ])await assertManifestFails(mutation,pattern);
  const fixture=await fixturePackage();assert.ok(Object.isFrozen(validateManifest(fixture.manifest).objects));
  await assert.rejects(()=>verifyPackage(encoder.encode(JSON.stringify(fixture.manifest,null,2)),fixture.objects,SUNTON_PROFILE),/Noncanonical/);
  const bom=new Uint8Array(fixture.manifestBytes.length+3);bom.set([239,187,191]);bom.set(fixture.manifestBytes,3);await assert.rejects(()=>verifyPackage(bom,fixture.objects,SUNTON_PROFILE));
  await assert.rejects(()=>verifyPackage(new Uint8Array(4097),fixture.objects,SUNTON_PROFILE),/budget/);
  await assert.rejects(()=>verifyPackage(fixture.manifestBytes,fixture.objects,SUNTON_PROFILE,'sha256:'+'0'.repeat(64)),/Package hash/);
  await assert.rejects(()=>verifyPackage(fixture.manifestBytes,fixture.objects,WAVESHARE_PROFILE),/[Pp]rofile/);
});
test('profile diagnostics identify exact unsupported feature and budget with dynamic headroom separate',async()=>{
  const fixture=await fixturePackage(),manifest=structuredClone(fixture.manifest);
  manifest.targetProfileId='other';manifest.rendererAbi=2;manifest.viewport.width=800;manifest.viewport.height=480;manifest.viewport.content=[0,0,800,480];manifest.requires.push('input.xy');manifest.counts.nodes=129;
  manifest.resources.internalRamBytes=25000;manifest.resources.psramBytes=1;manifest.resources.transferBytes=2049;manifest.resources.objectBytes=800000;manifest.objects[0].bytes=9000;manifest.objects[1].bytes=17000;
  const errors=checkProfile(manifest,SUNTON_PROFILE,4097,{internalRamBytes:100,psramBytes:0});
  for(const expected of['targetProfileId','rendererAbi','viewport.width','viewport.height','viewport.content','requires','counts.nodes','manifestBytes','packageBytes','resources.internalRamBytes','resources.psramBytes','resources.transferBytes','objects.semantics.bytes','objects.scene.bytes','headroom.internalRamBytes','headroom.psramBytes'])assert.ok(errors.some(error=>error.path===expected),expected);
  assert.match(errors.find(error=>error.path==='requires').message,/input.xy.*sunton/);
  const changed=structuredClone(SUNTON_PROFILE);changed.presentationHz++;assert.notEqual(await profileHash(changed),await profileHash(SUNTON_PROFILE));
});
test('verification checks actual object set, hashes, binary facts and peak RAM accounting',async()=>{
  const fixture=await fixturePackage();
  const missing=new Map(fixture.objects);missing.delete('fonts');await assert.rejects(()=>verifyPackage(fixture.manifestBytes,missing,SUNTON_PROFILE),/set/);
  const wrong=new Map(fixture.objects);wrong.set('scene',new Uint8Array(1));await assert.rejects(()=>verifyPackage(fixture.manifestBytes,wrong,SUNTON_PROFILE),/size/);
  const mutated=new Map(fixture.objects);const bytes=Uint8Array.from(mutated.get('scene'));bytes[bytes.length-1]^=1;mutated.set('scene',bytes);await assert.rejects(()=>verifyPackage(fixture.manifestBytes,mutated,SUNTON_PROFILE),/Object hash/);
  for(const mutate of[m=>m.counts.nodes++,m=>m.requires.push('scene.unsupported'),m=>m.resources.maxDirtyPixels++,m=>m.resources.snapshotBytes=0,m=>m.resources.tileBytes=0,m=>m.resources.transferBytes=1,m=>m.resources.internalRamBytes=4095,m=>m.resources.cacheBytes=8192]){
    const manifest=structuredClone(fixture.manifest);mutate(manifest);manifest.requires.sort();await assert.rejects(()=>verifyPackage(encoder.encode(canonical(manifest)),fixture.objects,SUNTON_PROFILE));
  }
  await assert.rejects(()=>packPackage(source,new Map(),SUNTON_PROFILE,memory),/set/);
  const unknown=new Map(fixture.objects);unknown.delete('fonts');unknown.set('bad',new Uint8Array(32));await assert.rejects(()=>packPackage(source,unknown,SUNTON_PROFILE,memory),/Unknown/);
  const a=await packPackage(source,fixture.objects,SUNTON_PROFILE,memory),b=await packPackage(source,new Map([...fixture.objects].reverse()),SUNTON_PROFILE,memory);assert.equal(a.packageHash,b.packageHash);
  const res=await packPackage({...source,compilerVersion:'1.0.1'},fixture.objects,SUNTON_PROFILE,memory);assert.notEqual(res.packageHash,a.packageHash);
});

const c=value=>({op:EXPR.constant,value}),v=index=>({op:EXPR.variable,operand:index}),op=op=>({op});
test('fixed-point conversion, formatting, deterministic rounding and invalid data',()=>{
  assert.equal(toFixed(1.5),98304);assert.equal(toFixed(-1/3),-21845);assert.throws(()=>toFixed(Infinity));assert.throws(()=>toFixed(32768));
  assert.equal(formatFixed(-32768,0),'-1');assert.equal(formatFixed(-1,0),'0');assert.equal(formatFixed(81920,1),'1.3');assert.equal(formatFixed(65536,4),'1.0000');assert.throws(()=>formatFixed(0,5));assert.throws(()=>formatFixed(1.1,1));
  for(const [operator,left,right,result]of[[EXPR.add,1,2,3],[EXPR.subtract,3,2,1],[EXPR.multiply,1.5,2,3],[EXPR.divide,-1,3,-21845/Q],[EXPR.minimum,1,2,1],[EXPR.maximum,1,2,2],[EXPR.less,1,2,1],[EXPR.equal,1,1,1],[EXPR.and,1,0,0],[EXPR.or,0,1,1]])assert.equal(evaluate([c(toFixed(left)),c(toFixed(right)),op(operator)],[]),toFixed(result));
  assert.equal(evaluate([c(0),op(EXPR.not)],[]),Q);assert.equal(evaluate([c(Q),op(EXPR.not)],[]),0);
  assert.equal(evaluate([c(Q),c(4),c(5),op(EXPR.select)],[]),4);assert.equal(evaluate([c(0),c(4),c(5),op(EXPR.select)],[]),5);
  assert.equal(evaluate([c(-5),c(0),c(10),op(EXPR.clamp)],[]),0);assert.equal(evaluate([c(20),c(0),c(10),op(EXPR.clamp)],[]),10);
  assert.equal(evaluate([v(0)],[Q]),Q);assert.equal(evaluate([v(0)],[null]),null);assert.equal(evaluate([v(0)],[NaN]),null);
  for(const instructions of[[c(1),c(0),op(EXPR.divide)],[c(2147483647),c(1),op(EXPR.add)],[c(2),op(EXPR.not)],[c(2),c(0),op(EXPR.and)],[c(2),c(0),op(EXPR.or)],[c(1),c(10),c(0),op(EXPR.clamp)],[],[op(EXPR.add)],[c(1),c(2)],[{op:255}],[...Array(9).fill(c(1)),...Array(8).fill(op(EXPR.add))]])assert.equal(evaluate(instructions,[]),null);
  assert.throws(()=>expressionStackDepth(Array(1025).fill(c(1))));
});
test('lookup clamps, interpolates with signed integer rounding and rejects invalid tables',()=>{
  const lookup=[[0,0],[Q,Q*2],[Q*2,Q]],expr=value=>[c(value),{op:EXPR.lookup,operand:0,value:3}];
  assert.equal(evaluate(expr(-Q),[],lookup),0);assert.equal(evaluate(expr(Q/2),[],lookup),Q);assert.equal(evaluate(expr(Q*2),[],lookup),Q);assert.equal(evaluate(expr(Q*3),[],lookup),Q);
  assert.equal(evaluate(expr(0),[],[[0,0],[0,Q],[Q,Q]]),null);assert.equal(evaluate(expr(0),[],[]),null);
  assert.equal(evaluate([c(0),{op:EXPR.lookup,operand:0,value:1}],[],lookup),null);
});
test('binding interval proof rejects understated reachable damage rather than trusting declared min/max',()=>{
  let objects=change(fixtureObjects(),'scene',4,data=>{data.setUint8(2,PROPERTY.translateX);data.setInt32(8,0,true);data.setInt32(12,0,true);});
  objects=change(objects,'scene',5,data=>{data.setUint8(0,EXPR.constant);data.setInt32(4,100*Q,true);});
  assert.throws(()=>inspect(objects),/proven expression range/);
  objects=change(fixtureObjects(),'scene',5,data=>data.setUint8(0,EXPR.constant));
  objects=change(objects,'scene',4,data=>{data.setUint8(2,PROPERTY.translateX);data.setInt32(12,0,true);});
  assert.equal(inspect(objects).maxDirtyPixels,1536);
  objects=change(objects,'scene',4,data=>{data.setInt32(8,-Q,true);});assert.throws(()=>inspect(objects),/reachable bounds/);
  objects=change(fixtureObjects(),'semantics',2,data=>data.setUint8(6,2));
  objects=change(objects,'scene',3,data=>data.setUint8(2,2));assert.throws(()=>inspect(objects),/proven boolean/);
});
test('interval and boolean proofs cover arithmetic, clamp, selector, lookup and failure cases',()=>{
  for(const [operator,left,right,result]of[
    [EXPR.add,[Q,2*Q],[Q,2*Q],[2*Q,4*Q]],[EXPR.subtract,[Q,2*Q],[Q,2*Q],[-Q,Q]],
    [EXPR.multiply,[-Q,Q],[Q,2*Q],[-2*Q,2*Q]],[EXPR.divide,[-Q,Q],[Q,2*Q],[-Q,Q]],
    [EXPR.minimum,[0,Q],[Q,2*Q],[0,Q]],[EXPR.maximum,[0,Q],[Q,2*Q],[Q,2*Q]],
    [EXPR.less,[0,Q],[Q,2*Q],[0,Q]],[EXPR.equal,[0,Q],[0,Q],[0,Q]],
  ])assert.deepEqual(expressionRange([v(0),v(1),op(operator)],[left,right]),result);
  assert.deepEqual(expressionRange([v(0),op(EXPR.not)],[[0,Q]]),[0,Q]);
  assert.deepEqual(expressionRange([v(0),c(3),c(4),op(EXPR.select)],[[0,Q]]),[3,4]);
  assert.deepEqual(expressionRange([c(Q),c(3),c(4),op(EXPR.select)],[]),[3,3]);
  assert.deepEqual(expressionRange([c(0),c(3),c(4),op(EXPR.select)],[]),[4,4]);
  assert.deepEqual(expressionRange([v(0),c(0),c(Q),op(EXPR.clamp)],[[-Q,Q*2]]),[0,Q]);
  assert.deepEqual(expressionRange([v(0),{op:EXPR.lookup,value:3}],[[Q/2,Q*2]],[[0,0],[Q,Q*2],[Q*2,Q]]),[Q,2*Q]);
  for(const [instructions,vars]of[[[v(0)],[]],[[c(0),c(-1),c(-2),op(EXPR.clamp)],[]],[[c(2147483647),c(1),op(EXPR.add)],[]],[[v(0),v(1),op(EXPR.divide)],[[0,Q],[-Q,Q]]],[[c(0),{op:EXPR.lookup,value:2}],[]]])assert.equal(expressionRange(instructions,vars),null);
  for(const [instructions,kinds,expected]of[
    [[c(0)],[],'boolean'],[[c(1)],[],'number'],[[v(0)],['boolean'],'boolean'],[[v(0)],[],null],
    [[v(0),op(EXPR.not)],['boolean'],'boolean'],[[v(0),op(EXPR.not)],['number'],null],
    [[c(0),{op:EXPR.lookup,value:2}],[],'number'],[[c(Q),c(0),c(Q),op(EXPR.select)],[],'boolean'],
    [[c(2),c(0),c(Q),op(EXPR.select)],[],null],[[c(0),c(1),c(2),op(EXPR.clamp)],[],'number'],
    [[c(0),c(Q),op(EXPR.and)],[],'boolean'],[[c(1),c(Q),op(EXPR.or)],[],null],
    [[c(0),c(Q),op(EXPR.less)],[],'boolean'],[[c(0),c(Q),op(EXPR.add)],[],'number'],
  ])assert.equal(expressionKind(instructions,kinds),expected);
});
test('bounded local timelines admit boolean steps and reject fractional visibility/interpolation',()=>{
  let objects=change(fixtureObjects(),'scene',4,(_data,_bytes,sections)=>{sections.find(item=>item.kind===4).data=new Uint8Array();sections.find(item=>item.kind===5).data=new Uint8Array();
    sections.push({kind:6,data:record(24,[[2,'Uint8',PROPERTY.visible],[6,'Uint16',2],[8,'Uint16',100]])});
    const keys=new Uint8Array(16);keys.set(record(8,[[4,'Int32',0]]));keys.set(record(8,[[0,'Uint16',100],[4,'Int32',Q]]),8);sections.push({kind:7,data:keys});
  });
  const facts=inspect(objects);assert.equal(facts.counts.timelines,1);assert.ok(facts.requires.includes('scene.timeline'));
  for(const [table,offset,type,value]of[[6,3,'Uint8',1],[6,6,'Uint16',9],[6,8,'Uint16',49],[6,10,'Uint16',2],[7,0,'Uint16',1],[7,4,'Int32',32768],[7,8,'Uint16',0],[7,10,'Uint16',1]])assert.throws(()=>inspect(change(objects,'scene',table,data=>data[`set${type}`](offset,value,true))));
});
test('numeric text formatting precision and complete range fit are validated',()=>{
  let objects=change(fixtureObjects(),'scene',4,data=>{data.setUint16(0,3,true);data.setUint8(2,PROPERTY.number);});
  assert.equal(inspect(objects).maxBlendPixelLayers,768);
  assert.throws(()=>inspect(change(objects,'scene',2,data=>data.setUint16(192+42,5,true))),/precision/);
  objects=change(objects,'scene',2,data=>{data.setUint16(192+42,4,true);data.setUint32(192+44,0,true);data.setUint16(192+48,0,true);data.setUint16(192+50,1,true);});
  assert.throws(()=>inspect(objects),/Formatted number/);
});
