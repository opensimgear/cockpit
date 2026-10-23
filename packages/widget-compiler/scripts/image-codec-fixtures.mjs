// Generated from original in-repository pixels; no third-party artwork.
import {readFileSync,writeFileSync} from 'node:fs';
import {rasterPng} from '../test/fixtures.mjs';
import jpegEncode,{init as initJpeg} from '@jsquash/jpeg/encode.js';
import webpEncode,{init as initWebp} from '@jsquash/webp/encode.js';
const jpegWasm=await WebAssembly.compile(readFileSync(new URL('../node_modules/@jsquash/jpeg/codec/enc/mozjpeg_enc.wasm',import.meta.url)));
const webpWasm=await WebAssembly.compile(readFileSync(new URL('../node_modules/@jsquash/webp/codec/enc/webp_enc.wasm',import.meta.url)));
await initJpeg(jpegWasm);await initWebp(webpWasm);
const image={width:2,height:2,data:new Uint8ClampedArray([255,0,0,255,0,255,0,255,0,0,255,255,255,255,255,255])};
const cases=[{mime:'image/png',base64:Buffer.from(rasterPng()).toString('base64')},{mime:'image/jpeg',base64:Buffer.from(await jpegEncode(image,{quality:90})).toString('base64')},{mime:'image/webp',base64:Buffer.from(await webpEncode(image,{quality:90})).toString('base64')}];
const path=new URL('../test/fixtures/image-codecs.json',import.meta.url),output=JSON.stringify({formatVersion:1,cases},null,2)+'\n';
if(process.argv.includes('--check')){if(readFileSync(path,'utf8')!==output)throw new Error('Image codec fixtures stale');}else writeFileSync(path,output);
