import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {compileWidget,clearWidgetCompileCache,decodeCompiledFrame,imageDimensions,prepareRasterImages,validateSourceImages,validateWidgetSource} from '../src/index.ts';
import {tinySource,rasterPng} from './fixtures.mjs';
const corpus=JSON.parse(readFileSync(new URL('./fixtures/image-codecs.json',import.meta.url),'utf8')).cases;
function source(image){const source=tinySource();source.assets=[{id:'artwork',...image}];source.scene=[{id:'image',kind:'image',x:0,y:0,width:32,height:32,assetId:'artwork'}];return source;}
function crc32(bytes){let crc=0xffffffff;for(const byte of bytes){crc^=byte;for(let bit=0;bit<8;bit++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);}return (crc^0xffffffff)>>>0;}
function badDeflatePng(){const bytes=rasterPng(),view=new DataView(bytes.buffer);let at=8;while(at+12<=bytes.length){const size=view.getUint32(at),type=String.fromCharCode(...bytes.subarray(at+4,at+8));if(type==='IDAT'){bytes[at+size+7]^=1;view.setUint32(at+8+size,crc32(bytes.subarray(at+4,at+8+size)));break;}at+=size+12;}return bytes;}

test('same pinned WASM decoders validate and normalize PNG/JPEG/WebP before resvg',async()=>{
  for(const image of corpus){const original=source(image);assert.equal(validateWidgetSource(original).ok,true);const normalized=await prepareRasterImages(original);assert.equal(original.assets[0].mime,image.mime);assert.equal(normalized.assets[0].mime,'image/png');assert.notEqual(normalized.assets[0],original.assets[0]);await validateSourceImages(original);
    const first=await compileWidget(original),pixels=decodeCompiledFrame(first.artifact,first.assets,first.artifact.frames[0].id);assert.equal(pixels[3],255,image.mime+' must render a real image');assert(pixels.some((value,index)=>index%4!==3&&value>0),image.mime+' must retain image color');clearWidgetCompileCache();const second=await compileWidget(original);assert.deepEqual(second,first);
  }
});

test('normalization preserves native ImageData prototype dimensions without relying on object spread',()=>{
  const entry=new URL('../src/index.ts',import.meta.url).href,fixture=source(corpus[0]);
  const code=`globalThis.ImageData=class {#data;#width;#height;constructor(data,width,height){this.#data=data;this.#width=width;this.#height=height;}get data(){return this.#data;}get width(){return this.#width;}get height(){return this.#height;}};const {compileWidget,decodeCompiledFrame}=await import(${JSON.stringify(entry)});const b=await compileWidget(${JSON.stringify(fixture)});const p=decodeCompiledFrame(b.artifact,b.assets,b.artifact.frames[0].id);if(p[3]!==255)throw Error('Native-shaped ImageData failed to produce visible pixels');`;
  const result=spawnSync(process.execPath,['--input-type=module','-e',code],{encoding:'utf8'});assert.equal(result.status,0,result.stderr);
});

test('PNG IDAT with valid chunk CRC but broken zlib checksum is rejected headlessly',async()=>{
  const corrupt=badDeflatePng();assert.deepEqual(imageDimensions(corrupt,'image/png'),{width:1,height:1});const bad=source({mime:'image/png',base64:Buffer.from(corrupt).toString('base64')});assert.equal(validateWidgetSource(bad).ok,true,'structural header validation alone is insufficient');await assert.rejects(compileWidget(bad),/failed full image\/png decoding/);await compileWidget(source(corpus[0]));
});

test('corrupt JPEG/WebP compressed content and truncated containers never flatten silently',async()=>{
  for(const image of corpus.filter(image=>image.mime!=='image/png')){
    const bytes=Uint8Array.from(Buffer.from(image.base64,'base64'));let corrupt;
    if(image.mime==='image/webp'){corrupt=bytes.slice(0,-4);const view=new DataView(corrupt.buffer);view.setUint32(4,corrupt.length-8,true);view.setUint32(16,view.getUint32(16,true)-4,true);}
    else{corrupt=Uint8Array.from(bytes);let at=2;while(at+4<corrupt.length){while(corrupt[at]===255)at++;const marker=corrupt[at++],size=(corrupt[at]<<8)|corrupt[at+1];if(marker===218){corrupt.fill(0,at+size,corrupt.length-2);break;}at+=size;}}
    const bad=source({mime:image.mime,base64:Buffer.from(corrupt).toString('base64')});assert.equal(validateWidgetSource(bad).ok,true);await assert.rejects(compileWidget(bad),/failed full/);await compileWidget(source(image));
    const truncated=source({mime:image.mime,base64:Buffer.from(bytes.slice(0,-2)).toString('base64')});await assert.rejects(compileWidget(truncated));
  }
});

test('bounded image processing retains cancellation and ignores optional audio assets',async()=>{
  const controller=new AbortController();controller.abort();await assert.rejects(prepareRasterImages(source(corpus[0]),controller.signal),{name:'AbortError'});
  const empty=tinySource();assert.deepEqual(await prepareRasterImages(empty),empty);empty.assets=[{id:'not-image',mime:'audio/wav',base64:'ignored-by-image-stage'}];await validateSourceImages(empty);
});
