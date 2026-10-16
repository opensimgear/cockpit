import { writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { DIRECTORY_BYTES, EXPR, HEADER_BYTES, NONE, OP, RECORD_BYTES, SUNTON_PROFILE, WAVESHARE_PROFILE, canonical, digest, packBinaryObject, packPackage } from '../src/index.ts';

const encoder = new TextEncoder();
export function record(size, values) {
  const bytes = new Uint8Array(size), view = new DataView(bytes.buffer);
  for (const [offset, type, value] of values) view[`set${type}`](offset, value, true);
  return bytes;
}
export function fixtureObjects(profile = SUNTON_PROFILE) {
  const strings = encoder.encode('firetargetlightpowerWmainFIRE');
  const controls = record(32, [[0,'Uint32',0],[4,'Uint16',4],[6,'Uint8',1],[8,'Uint32',4],[12,'Uint16',6],[20,'Int32',65536],[28,'Uint16',0],[30,'Uint16',NONE]]);
  const outputs = record(32, [[0,'Uint32',10],[4,'Uint16',5],[6,'Uint8',1],[8,'Uint32',15],[12,'Uint16',5],[20,'Int32',65536],[24,'Uint32',20],[28,'Uint16',1]]);
  const interaction = record(24, [[0,'Uint32',0],[4,'Uint16',4],[6,'Uint8',1],[8,'Uint16',0],[10,'Uint16',NONE],[12,'Uint16',NONE],[14,'Uint16',NONE]]);
  const semantics = packBinaryObject('semantics', [{kind:1,data:controls},{kind:2,data:outputs},{kind:3,data:interaction},{kind:4,data:strings}]);
  const pages = record(16, [[0,'Uint32',0],[4,'Uint16',4],[8,'Uint16',4],[10,'Uint16',0x1082]]);
  const makeNode = (op,x,y,w,h,fields=[]) => record(64, [[2,'Uint8',op],[3,'Uint8',1],[4,'Int16',x],[6,'Int16',y],[8,'Uint16',w],[10,'Uint16',h],[12,'Int16',profile.content[0]],[14,'Int16',profile.content[1]],[16,'Uint16',profile.content[2]],[18,'Uint16',profile.content[3]],[20,'Int16',x],[22,'Int16',y],[24,'Uint16',w],[26,'Uint16',h],[28,'Uint16',NONE],[30,'Uint16',NONE],[32,'Uint16',NONE],[36,'Uint16',0x07ff],...fields]);
  const nodeValues = [makeNode(OP.rect,8,32,48,24),makeNode(OP.image565,64,32,2,2,[[28,'Uint16',0]]),makeNode(OP.mask4,68,32,2,2,[[30,'Uint16',1]]),makeNode(OP.text,8,64,48,16,[[32,'Uint16',0],[44,'Uint32',4],[48,'Uint16',4],[50,'Uint16',16]])];
  const nodes = new Uint8Array(nodeValues.length*64); nodeValues.forEach((node,index)=>nodes.set(node,index*64));
  const vars = record(24, [[2,'Uint8',1],[12,'Int32',65536]]);
  const bindings = record(16, [[0,'Uint16',0],[2,'Uint8',1],[6,'Uint16',1],[12,'Int32',65536]]);
  const expressions = record(16, [[0,'Uint8',EXPR.variable]]);
  const hits = record(24, [[4,'Int16',8],[6,'Int16',32],[8,'Uint16',48],[10,'Uint16',24]]);
  const tileEntries = new Map();
  nodeValues.forEach((node,index)=>{
    const view=new DataView(node.buffer),x=view.getInt16(4,true),y=view.getInt16(6,true),w=view.getUint16(8,true),h=view.getUint16(10,true);
    for(let ty=Math.floor((y-profile.content[1])/8);ty<=Math.floor((y+h-1-profile.content[1])/8);ty++)for(let tx=Math.floor(x/64);tx<=Math.floor((x+w-1)/64);tx++){
      const key=ty*256+tx,entry=tileEntries.get(key)??{x:tx,y:ty,members:[]};entry.members.push(index);tileEntries.set(key,entry);
    }
  });
  const entries=[...tileEntries.entries()].sort((a,b)=>a[0]-b[0]).map(([,value])=>value), tiles=new Uint8Array(entries.length*12), membership=[];
  entries.forEach((tile,index)=>{tiles.set(record(12,[[2,'Uint16',tile.x],[4,'Uint16',tile.y],[6,'Uint16',membership.length],[8,'Uint16',tile.members.length]]),index*12);membership.push(...tile.members);});
  const tileNodes=new Uint8Array(membership.length*2);membership.forEach((node,index)=>new DataView(tileNodes.buffer).setUint16(index*2,node,true));
  const scene=packBinaryObject('scene',[{kind:1,data:pages},{kind:2,data:nodes},{kind:3,data:vars},{kind:4,data:bindings},{kind:5,data:expressions},{kind:8,data:hits},{kind:10,data:encoder.encode('mainFIRE')},{kind:11,data:tiles},{kind:12,data:tileNodes}]);
  const imageRecords=new Uint8Array(48);imageRecords.set(record(24,[[0,'Uint16',2],[2,'Uint16',2],[4,'Uint32',4],[12,'Uint32',8],[16,'Uint8',1]]));
  imageRecords.set(record(24,[[0,'Uint16',2],[2,'Uint16',2],[4,'Uint32',1],[8,'Uint32',8],[12,'Uint32',2],[16,'Uint8',2]]),24);
  const images=packBinaryObject('images',[{kind:1,data:imageRecords},{kind:2,data:Uint8Array.from([0xff,0x07,0xff,0xff,0x00,0xf8,0x1f,0x00,0xf0,0x8f])}]);
  const fontRecords=record(16,[[2,'Uint16',1],[6,'Uint16',16],[8,'Int16',12]]), glyphs=record(24,[[0,'Uint32',63],[4,'Uint16',2],[6,'Uint16',2],[12,'Int16',3],[20,'Uint32',2]]);
  const fonts=packBinaryObject('fonts',[{kind:1,data:fontRecords},{kind:2,data:glyphs},{kind:3,data:Uint8Array.from([0xff,0x88])}]);
  return new Map([['semantics',semantics],['scene',scene],['images',images],['fonts',fonts]]);
}
export async function fixturePackage(profile=SUNTON_PROFILE) {
  return packPackage({sourceUiId:'fixture-panel',sourcePublicationHash:await digest(encoder.encode('source-publication-fixture')),semanticFingerprint:await digest(encoder.encode('semantic-fingerprint-fixture')),compilerId:'osg-render-compiler',compilerVersion:'0.1.0'},fixtureObjects(profile),profile,{internalRamBytes:8192,psramBytes:0,sceneRamBytes:0,snapshotBytes:1024,tileBytes:1024,transferBytes:2048,cacheBytes:0});
}
export async function writeFixtures() {
  const dir=new URL('./fixtures/',import.meta.url),sunton=await fixturePackage(),waveshare=await fixturePackage(WAVESHARE_PROFILE);
  for(const [kind,bytes] of sunton.objects)await writeFile(new URL(`${kind}.bin`,dir),bytes);
  await writeFile(new URL('waveshare-scene.bin',dir),waveshare.objects.get('scene'));
  await writeFile(new URL('sunton-profile.json',dir),canonical(SUNTON_PROFILE));
  await writeFile(new URL('waveshare-profile.json',dir),canonical(WAVESHARE_PROFILE));
  await writeFile(new URL('manifest.json',dir),sunton.manifestBytes);
  await writeFile(new URL('waveshare-manifest.json',dir),waveshare.manifestBytes);
  await writeFile(new URL('golden.json',dir),JSON.stringify({packageHash:sunton.packageHash,profileHash:sunton.manifest.targetProfileHash,wavesharePackageHash:waveshare.packageHash,waveshareProfileHash:waveshare.manifest.targetProfileHash},null,2)+'\n');
  await writeFile(new URL('abi.json',dir),JSON.stringify({headerBytes:HEADER_BYTES,directoryBytes:DIRECTORY_BYTES,recordBytes:RECORD_BYTES},null,2)+'\n');
}
if(process.argv[1]===fileURLToPath(import.meta.url))await writeFixtures();
