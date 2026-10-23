/// <reference path="../node_modules/@jsquash/jpeg/emscripten-types.d.ts" />
import {decodeWidgetBase64,encodeWidgetBase64,widgetDigest} from '../../widget-runtime/src/artifact.ts';
import {assertSource,imageDimensions} from './source.ts';
import {loadCompilerResource} from './resources.ts';
import type {WidgetSource} from './types.ts';

type Decoded={width:number;height:number;data:Uint8ClampedArray};
type Decoder=(bytes:ArrayBuffer)=>Promise<Decoded>;
const PNG_WASM=new URL('../node_modules/@jsquash/png/codec/pkg/squoosh_png_bg.wasm',import.meta.url);
const JPEG_WASM=new URL('../node_modules/@jsquash/jpeg/codec/dec/mozjpeg_dec.wasm',import.meta.url);
const WEBP_WASM=new URL('../node_modules/@jsquash/webp/codec/dec/webp_dec.wasm',import.meta.url);
export const IMAGE_COMPILER_ID='png-3.1.1-jpeg-1.6.0-webp-1.5.0';
const decoders=new Map<string,Promise<Decoder>>(),validated=new Map<string,{mime:'image/png';base64:string}>();
let pngEncoder:Promise<(image:Decoded)=>Promise<ArrayBuffer>>|undefined;
let imageCacheBytes=0;
function cancelled(signal?:AbortSignal):void{if(signal?.aborted)throw new DOMException('Image validation cancelled','AbortError');}
async function pinned(url:URL,hash:string):Promise<ArrayBuffer>{const bytes=await loadCompilerResource(url);assertSource(await widgetDigest(bytes)===hash,'Image codec WASM hash differs from the compiler identity');return Uint8Array.from(bytes).buffer;}
function queuedDecoder(decode:Decoder,warnings:string[]):Decoder{let pending:Promise<unknown>=Promise.resolve();return bytes=>{const next=pending.then(async()=>{warnings.length=0;const image=await decode(bytes);assertSource(warnings.length===0,'Image decoder reported corrupt data: '+warnings.join('; '));return image;});pending=next.catch(()=>undefined);return next;};}
/** PNG's image decoder tolerates a bad zlib Adler checksum. Validate its stream
 * separately, with a strict size bound from the header, before decoding pixels. */
async function pngStream(bytes:Uint8Array,signal?:AbortSignal):Promise<void>{
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),width=view.getUint32(16),height=view.getUint32(20),depth=bytes[24],color=bytes[25],channels=({0:1,2:3,3:1,4:2,6:4} as Record<number,number>)[color];
  const depths:Record<number,number[]>={0:[1,2,4,8,16],2:[8,16],3:[1,2,4,8],4:[8,16],6:[8,16]};assertSource(channels&&depths[color]?.includes(depth)&&bytes[26]===0&&bytes[27]===0&&(bytes[28]===0||bytes[28]===1),'Unsupported PNG pixel format');
  const row=(w:number)=>Math.ceil(w*channels*depth/8)+1;let expected=0;
  if(bytes[28]===0)expected=row(width)*height;
  else for(const [x,y,dx,dy] of [[0,0,8,8],[4,0,8,8],[0,4,4,8],[2,0,4,4],[0,2,2,4],[1,0,2,2],[0,1,1,2]]){const w=Math.max(0,Math.ceil((width-x)/dx)),h=Math.max(0,Math.ceil((height-y)/dy));if(w&&h)expected+=row(w)*h;}
  let cursor=8;const data:Uint8Array[]=[];while(cursor+12<=bytes.length){const size=view.getUint32(cursor);if(String.fromCharCode(...bytes.subarray(cursor+4,cursor+8))==='IDAT')data.push(Uint8Array.from(bytes.subarray(cursor+8,cursor+8+size)));cursor+=size+12;}
  const stream=new Blob(data.map(bytes=>Uint8Array.from(bytes).buffer)).stream().pipeThrough(new DecompressionStream('deflate')),reader=stream.getReader();let received=0;
  try{for(;;){cancelled(signal);const next=await reader.read();if(next.done)break;received+=next.value.length;assertSource(received<=expected,'PNG decompressed pixels exceed declared dimensions');}assertSource(received===expected,'PNG decompressed pixels are incomplete');}
  catch(error){await reader.cancel().catch(()=>undefined);throw error;}finally{reader.releaseLock();}
}
async function decoder(mime:string):Promise<Decoder>{
  let ready=decoders.get(mime);if(!ready){ready=(async()=>{
    if(mime==='image/png'){const png=await import('@jsquash/png/decode.js');await png.init(await pinned(PNG_WASM,'263d6e658808a74b72a1a99c5cc1d619237e70c150db6e41d5d84d3d117ab9be'));return png.default;}
    const module=await WebAssembly.compile(await pinned(mime==='image/jpeg'?JPEG_WASM:WEBP_WASM,mime==='image/jpeg'?'a7c4b12169817e779ff4af137981393ae924944e167ad1bd95747c9199162d3e':'30fb52fa2a80166d25ba7debf902218904ba1f05ccce9f959f722beff9e2f344'));
    // Supplying the exact module avoids environment-specific Node/file fetches.
    // The upstream init helper forwards this official Emscripten hook unchanged.
    const warnings:string[]=[],options={printErr:(message:string)=>warnings.push(message),instantiateWasm(imports:WebAssembly.Imports,callback:(module:WebAssembly.Module)=>void){const instance=new WebAssembly.Instance(module,imports);(callback as unknown as (instance:WebAssembly.Instance)=>void)(instance);return instance.exports;}};
    if(mime==='image/jpeg'){const jpeg=await import('@jsquash/jpeg/decode.js');await jpeg.init(options);return queuedDecoder((bytes:ArrayBuffer)=>jpeg.default(bytes,{preserveOrientation:false}),warnings);}
    const webp=await import('@jsquash/webp/decode.js');await webp.init(options);return queuedDecoder(webp.default,warnings);
  })().catch(error=>{decoders.delete(mime);throw error;});decoders.set(mime,ready);}return ready;
}
/** Full pinned WASM decoding is validation only: resvg remains the sole renderer.
 * Header admission runs first to bound decoder dimensions and allocation. */
export async function prepareRasterImages(source:WidgetSource,signal?:AbortSignal):Promise<WidgetSource>{
  const normalized=structuredClone(source);let normalizedBytes=0;
  for(const asset of normalized.assets??[]){if(!asset.mime.startsWith('image/'))continue;cancelled(signal);const bytes=decodeWidgetBase64(asset.base64),expected=imageDimensions(bytes,asset.mime),key=IMAGE_COMPILER_ID+':'+asset.mime+':'+await widgetDigest(bytes);cancelled(signal);const cached=validated.get(key);if(cached){asset.mime=cached.mime;asset.base64=cached.base64;normalizedBytes+=asset.base64.length;assertSource(normalizedBytes<=32*1024*1024,'Normalized source image byte budget exceeded');continue;}
    try{
      if(asset.mime==='image/png')await pngStream(bytes,signal);
      const decode=await decoder(asset.mime);cancelled(signal);const image=await decode(Uint8Array.from(bytes).buffer);cancelled(signal);
      assertSource(image&&image.width===expected.width&&image.height===expected.height&&image.data instanceof Uint8ClampedArray&&image.data.length===image.width*image.height*4,'Source image decoded dimensions or pixel buffer mismatch');
      const encode=await (pngEncoder??=(async()=>{await decoder('image/png');const png=await import('@jsquash/png/encode.js');await png.init();return (image:Decoded)=>png.default({width:image.width,height:image.height,data:Uint8ClampedArray.from(image.data),colorSpace:'srgb'});})());
      const png=await encode(image);cancelled(signal);asset.mime='image/png';asset.base64=encodeWidgetBase64(new Uint8Array(png));
    }catch(error){if(error instanceof DOMException&&error.name==='AbortError')throw error;throw new Error(`Source image ${asset.id} failed full ${asset.mime} decoding: ${error instanceof Error?error.message:String(error)}`);}
    normalizedBytes+=asset.base64.length;assertSource(normalizedBytes<=32*1024*1024,'Normalized source image byte budget exceeded');
    validated.set(key,{mime:'image/png',base64:asset.base64});imageCacheBytes+=asset.base64.length;while(validated.size>256||imageCacheBytes>16*1024*1024){const oldest=validated.keys().next().value!;imageCacheBytes-=validated.get(oldest)!.base64.length;validated.delete(oldest);}
  }
  return normalized;
}
export async function validateSourceImages(source:WidgetSource,signal?:AbortSignal):Promise<void>{await prepareRasterImages(source,signal);}
