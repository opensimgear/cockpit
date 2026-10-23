import { initWasm, Resvg } from '@resvg/resvg-wasm';
import {
  ARTIFACT_LIMITS, BROWSER_WIDGET_CAPABILITIES, RASTER_FORMAT, TILE_BYTES, TILE_SIZE,
  canonicalWidget, decodeWidgetBase64, encodeWidgetBase64, validateCompiledArtifact,
  validateWidgetCapabilities, widgetDigest,
  type CompiledAssetDescriptor, type CompiledAssetPayload, type CompiledDomain,
  type CompiledFrame, type CompiledMotion, type CompiledSelector, type CompiledTile,
  type CompiledWidgetArtifact, type CompiledWidgetBundle,
} from '../../widget-runtime/src/artifact.ts';
import type { Scalar } from '../../widget-runtime/src/behavior.ts';
import { SOURCE_LIMITS, assertSource, checkedSource, expressionRefs, resolvedParameters, sourceNumber, sourceToSvg } from './source.ts';
import type { CompileOptions, MotionTrack, WidgetMotion, WidgetSource, VisualNode } from './types.ts';
import {loadCompilerResource} from './resources.ts';
import {prepareRasterImages,IMAGE_COMPILER_ID} from './images.ts';

const FONTS = [
  { url: new URL('../../companion-ui/assets/fonts/osg-sans.ttf', import.meta.url), hash: '94bbd25a18ca665999feb05a537de9fd2b860dcfb78bbe9ca00270825bf235da' },
  { url: new URL('../../companion-ui/assets/fonts/osg-mono.ttf', import.meta.url), hash: '6a3412f058c7d8dfd9170c41e85ade48e5156ecb89356110ca57a0a27734af46' },
];
const WASM_URL = new URL('../node_modules/@resvg/resvg-wasm/index_bg.wasm', import.meta.url);
let ready: Promise<void> | undefined;
let fontBuffers: Uint8Array[] = [];
/** Default resources are pinned and shared by headless tests and Cockpit. */
export async function initializeWidgetCompiler(resources?: { fonts?: Uint8Array[] }): Promise<void> {
  assertSource(!resources||Object.keys(resources).every(key=>key==='fonts'),'Compiler resource overrides only accept the verified font bank');
  if(!ready)ready=(async()=>{
    const fonts=resources?.fonts??await Promise.all(FONTS.map(font=>loadCompilerResource(font.url)));
    assertSource(fonts.length===2,'Compiler needs its pinned font bank');
    for(let index=0;index<fonts.length;index++)assertSource(await widgetDigest(fonts[index])===FONTS[index].hash,'Compiler font hash differs; update compiler identity before changing fonts');
    const wasm=await loadCompilerResource(WASM_URL);assertSource(await widgetDigest(wasm)==='22bf6e9f9a100d972da0411a69c5ba504367fc1fa87b3b64e3f35e53926d2d70','Compiler rasterizer hash differs; update compiler identity before changing WASM');
    fontBuffers=fonts.map(bytes=>Uint8Array.from(bytes));await initWasm(wasm.buffer);
  })().catch(error=>{ready=undefined;throw error;});
  return ready;
}
function abort(signal?:AbortSignal):void{if(signal?.aborted)throw new DOMException('Widget compilation cancelled','AbortError');}
function freeze<T>(value:T):T{if(value&&typeof value==='object'){for(const item of Object.values(value))freeze(item);Object.freeze(value);}return value;}
const cache=new Map<string,CompiledWidgetBundle>();
export function clearWidgetCompileCache():void{cache.clear();}
export interface CompilePreflight { source:WidgetSource; parameters:Record<string,Scalar>; domains:CompiledDomain[]; selectors:CompiledSelector[]; width:number;height:number; baseFrames:number; maximumFrames:number; rawFrameBytes:number; }
/** Enumerate every accepted appearance before spending rasterization work. */
export function preflightWidget(source:WidgetSource,options:CompileOptions={}):CompilePreflight{
  const s=checkedSource(source,options),capabilities=options.capabilities??BROWSER_WIDGET_CAPABILITIES,checked=validateWidgetCapabilities(capabilities);assertSource(checked.ok,checked.ok?'':checked.error);
  const width=options.width??s.size.width,height=options.height??s.size.height;sourceNumber(width,1,capabilities.maxWidth,true);sourceNumber(height,1,capabilities.maxHeight,true);
  const parameters=resolvedParameters(s,options.parameters),references=expressionRefs(s.scene);const domains:CompiledDomain[]=[];
  for(const motion of s.motions??[])for(const signal of [...Object.keys(motion.from),...Object.keys(motion.to)])references.refs.add(signal);
  for(const signal of s.behavior.signals){
    if(!references.refs.has(signal.id))continue;
    const declared=s.visualDomains.find(domain=>domain.signal===signal.id);let values:Scalar[];let error=0;
    if(declared?.values){values=[...declared.values];if(signal.type==='number'){values.sort((a,b)=>Number(a)-Number(b));const maxGap=Math.max(...values.slice(1).map((value,index)=>Number(value)-Number(values[index])));error=maxGap/(signal.max-signal.min)*declared.travelPixels!*Math.max(width/s.size.width,height/s.size.height)/2;assertSource(error<=declared.maxErrorPixels!+1e-9,'Visual sampling exceeds accepted pixel error for '+signal.id);}}
    else if(signal.type==='boolean')values=[false,true];else if(signal.type==='enum')values=[...signal.values!];
    else{assertSource(declared,'Unbounded numeric visual signal: '+signal.id);const count=declared.samples!,min=declared.min!,max=declared.max!;values=Array.from({length:count},(_,index)=>min+(max-min)*index/(count-1));error=(declared.travelPixels!/(count-1))*Math.max(width/s.size.width,height/s.size.height)/2;assertSource(error<=declared.maxErrorPixels!+1e-9,'Visual sampling exceeds accepted pixel error for '+signal.id);}
    domains.push({signal:signal.id,values,numeric:signal.type==='number',maxErrorPixels:error,smoothingMs:(options.motion??true)&&capabilities.motion?declared?.smoothingMs??0:0});
  }
  let count=1;for(const domain of domains){count*=domain.values.length;assertSource(Number.isSafeInteger(count)&&count<=SOURCE_LIMITS.combinations,'Visual state combination budget exceeded');}
  // Regions are part of the presentation selector even if their artwork deduplicates.
  for(const region of s.behavior.regions){count*=region.states.length;assertSource(count<=SOURCE_LIMITS.combinations,'Visual region combination budget exceeded');}
  const selectors:CompiledSelector[]=[{values:{},regions:{}}];
  for(const domain of domains){const before=selectors.splice(0);for(const selector of before)for(const value of domain.values)selectors.push({values:{...selector.values,[domain.signal]:value},regions:{...selector.regions}});}
  for(const region of s.behavior.regions){const before=selectors.splice(0);for(const selector of before)for(const state of region.states)selectors.push({values:{...selector.values},regions:{...selector.regions,[region.id]:state}});}
  let extra=0;if((options.motion??true)&&capabilities.motion)for(const motion of s.motions??[])extra+=motionPairs(motion,selectors).length*Math.max(0,Math.ceil(motion.durationMs*motion.fps/1000)-1);
  const maximumFrames=count+extra;assertSource(maximumFrames<=capabilities.maxFrames,'Compiled frame count exceeds target budget');
  const rawFrameBytes=maximumFrames*width*height*3;assertSource(Number.isSafeInteger(rawFrameBytes)&&rawFrameBytes<=1024*1024*1024,'Uncompressed frame family exceeds compiler work budget');
  return {source:s,parameters,domains,selectors,width,height,baseFrames:count,maximumFrames,rawFrameBytes};
}
function matches(values:Record<string,Scalar>,expected:Record<string,Scalar>):boolean{return Object.entries(expected).every(([key,value])=>values[key]===value);}
function motionPairs(motion:WidgetMotion,selectors:CompiledSelector[]):{from:number;to:number}[]{
  const changed=new Set([...Object.keys(motion.from),...Object.keys(motion.to)]),changedRegions=new Set([...Object.keys(motion.fromRegions??{}),...Object.keys(motion.toRegions??{})]);const pairs:{from:number;to:number}[]=[];
  selectors.forEach((from,index)=>{if(!matches(from.values,motion.from)||!matches(from.regions,motion.fromRegions??{}))return;const target=selectors.findIndex(to=>matches(to.values,motion.to)&&matches(to.regions,motion.toRegions??{})&&Object.keys(from.regions).every(key=>changedRegions.has(key)||from.regions[key]===to.regions[key])&&Object.keys(from.values).every(key=>changed.has(key)||from.values[key]===to.values[key]));if(target>=0)pairs.push({from:index,to:target});});return pairs;
}
function easing(value:number,kind:MotionTrack['easing']):number{return kind==='ease-in'?value*value:kind==='ease-out'?1-(1-value)*(1-value):kind==='ease-in-out'?value<.5?2*value*value:1-((-2*value+2)**2)/2:value;}
export function sampleMotionTrack(track:MotionTrack,progress:number):number{
  const time=Math.max(0,Math.min(1,progress));let index=track.keyframes.findIndex(frame=>frame.at>=time);if(index<=0)return track.keyframes[0].value;const before=track.keyframes[index-1],after=track.keyframes[index],fraction=easing((time-before.at)/(after.at-before.at),track.easing);return before.value+(after.value-before.value)*fraction;
}
function withMotion(source:WidgetSource,motion:WidgetMotion,progress:number):WidgetSource{
  const changed=structuredClone(source),nodes=new Map<string,VisualNode>();function visit(scene:VisualNode[]):void{for(const node of scene){nodes.set(node.id,node);if(node.kind==='group')visit(node.children);}}visit(changed.scene);
  for(const track of motion.tracks){const node=nodes.get(track.node)! as unknown as Record<string,any>,parts=track.property.split('.');if(parts.length===2){node[parts[0]]??={};node[parts[0]][parts[1]]=sampleMotionTrack(track,progress);}else node[parts[0]]=sampleMotionTrack(track,progress);}return changed;
}
function rasterize(source:WidgetSource,selector:CompiledSelector,preflight:CompilePreflight):Uint8Array{
  const defaults=Object.fromEntries(source.behavior.signals.map(signal=>[signal.id,signal.initial]));
  const svg=sourceToSvg(source,{values:{...defaults,...selector.values},regions:selector.regions,parameters:preflight.parameters,width:preflight.width,height:preflight.height});
  const renderer=new Resvg(svg,{font:{fontBuffers,defaultFontFamily:'Rajdhani',sansSerifFamily:'Rajdhani',monospaceFamily:'IBM Plex Mono'},dpi:96,languages:['en'],shapeRendering:2,textRendering:2,imageRendering:0});
  try{const image=renderer.render();try{
    assertSource(image.width===preflight.width&&image.height===preflight.height,'Rasterizer returned unexpected dimensions');
    // resvg exposes premultiplied tiny-skia pixels. The deployable A8 plane is
    // straight alpha, so restore RGB before RGB565 quantization and composition.
    const pixels=Uint8Array.from(image.pixels);for(let offset=0;offset<pixels.length;offset+=4){const alpha=pixels[offset+3];if(alpha>0&&alpha<255)for(let channel=0;channel<3;channel++)pixels[offset+channel]=Math.min(255,Math.round(pixels[offset+channel]*255/alpha));}
    return pixels;
  }finally{image.free();}}finally{renderer.free();}
}
/** RGB565 LE plane (512 bytes), then straight alpha A8 plane (256 bytes). */
export function encodePixelTile(rgba:Uint8Array,width:number,height:number,left:number,top:number):Uint8Array{
  const tile=new Uint8Array(TILE_BYTES);for(let y=0;y<TILE_SIZE;y++)for(let x=0;x<TILE_SIZE;x++){
    if(left+x>=width||top+y>=height)continue;const source=((top+y)*width+left+x)*4,index=y*TILE_SIZE+x,alpha=rgba[source+3];if(alpha===0)continue;
    const rgb=(Math.floor((rgba[source]*31+127)/255)<<11)|(Math.floor((rgba[source+1]*63+127)/255)<<5)|Math.floor((rgba[source+2]*31+127)/255);tile[index*2]=rgb&255;tile[index*2+1]=rgb>>8;tile[512+index]=alpha;
  }return tile;
}
export async function compileWidget(source:WidgetSource,options:CompileOptions={}):Promise<CompiledWidgetBundle>{
  abort(options.signal);const preflight=preflightWidget(source,options),s=preflight.source,capabilities=structuredClone(options.capabilities??BROWSER_WIDGET_CAPABILITIES);
  const sourceHash=await widgetDigest(new TextEncoder().encode(canonicalWidget(s))),parameterHash=await widgetDigest(new TextEncoder().encode(canonicalWidget(preflight.parameters))),behaviorHash=await widgetDigest(new TextEncoder().encode(canonicalWidget(s.behavior))),capabilityHash=await widgetDigest(new TextEncoder().encode(canonicalWidget(capabilities)));
  const key=canonicalWidget({sourceHash,parameterHash,capabilityHash,width:preflight.width,height:preflight.height,motion:options.motion??true,audio:options.audio??true,compiler:'osg-widget-compiler-1',fonts:FONTS.map(font=>font.hash),images:IMAGE_COMPILER_ID});
  abort(options.signal);const cached=cache.get(key);if(cached){options.onProgress?.({completed:cached.artifact.frames.length,total:cached.artifact.frames.length,phase:'cached'});return cached;}
  const rasterSource=await prepareRasterImages(s,options.signal);await initializeWidgetCompiler();abort(options.signal);
  const tileBytes:Uint8Array[]=[],tileDigests:string[]=[],tileIndex=new Map<string,number>(),frames:CompiledFrame[]=[],motions:CompiledMotion[]=[],fallbacks:CompiledWidgetArtifact['fallbacks']=[];
  async function frame(pixels:Uint8Array,id:string,selector?:CompiledSelector):Promise<void>{
    abort(options.signal);const indices:number[]=[];for(let top=0;top<preflight.height;top+=TILE_SIZE)for(let left=0;left<preflight.width;left+=TILE_SIZE){const bytes=encodePixelTile(pixels,preflight.width,preflight.height,left,top),digest=await widgetDigest(bytes);let index=tileIndex.get(digest);if(index===undefined){index=tileBytes.length;assertSource(index<ARTIFACT_LIMITS.tiles,'Pixel dictionary tile budget exceeded');tileIndex.set(digest,index);tileBytes.push(bytes);tileDigests.push(digest);}indices.push(index);}
    frames.push({id,tiles:indices,...(selector?{selector}: {})});options.onProgress?.({completed:frames.length,total:preflight.maximumFrames,phase:'rasterizing'});await new Promise<void>(resolve=>setTimeout(resolve,0));abort(options.signal);
  }
  for(let index=0;index<preflight.selectors.length;index++)await frame(rasterize(rasterSource,preflight.selectors[index],preflight),'frame-'+String(index).padStart(4,'0'),preflight.selectors[index]);
  const motionEnabled=(options.motion??true)&&capabilities.motion;
  if(((s.motions?.length??0)>0||s.visualDomains.some(domain=>(domain.smoothingMs??0)>0))&&!motionEnabled)fallbacks.push({feature:'motion',reason:options.motion===false?'Motion and smoothing disabled by authoring preference; live values select static frames.':'Target does not advertise animation support; live values select static frames without smoothing.'});
  if(motionEnabled)for(const motion of s.motions??[]){for(const pair of motionPairs(motion,preflight.selectors)){
    assertSource(motions.length<128,'Too many compiled transition routes');const fromFrameId=frames[pair.from].id,toFrameId=frames[pair.to].id,sequence=[fromFrameId],steps=Math.ceil(motion.durationMs*motion.fps/1000),id='motion-'+String(motions.length).padStart(3,'0');
    for(let step=1;step<steps;step++){const frameId=id+'-'+String(step).padStart(3,'0');await frame(rasterize(withMotion(rasterSource,motion,step/steps),preflight.selectors[pair.from],preflight),frameId);sequence.push(frameId);}sequence.push(toFrameId);motions.push({id,fromFrameId,toFrameId,frames:sequence,durationMs:motion.durationMs,interruption:motion.interruption??'snap'});
  }}
  const descriptors:CompiledAssetDescriptor[]=[],payloads:CompiledAssetPayload[]=[],tiles:CompiledTile[]=[];
  const tilesPerAsset=Math.floor(Math.min(capabilities.maxAssetBytes,ARTIFACT_LIMITS.assetBytes)/TILE_BYTES);
  for(let start=0;start<tileBytes.length;start+=tilesPerAsset){abort(options.signal);const end=Math.min(tileBytes.length,start+tilesPerAsset),bytes=new Uint8Array((end-start)*TILE_BYTES);for(let index=start;index<end;index++)bytes.set(tileBytes[index],(index-start)*TILE_BYTES);const digest=await widgetDigest(bytes),assetId='pixels-'+digest.slice(0,32);descriptors.push({id:assetId,sha256:digest,sizeBytes:bytes.length,mime:'application/vnd.opensimgear.raster-tiles'});payloads.push({assetId,base64:encodeWidgetBase64(bytes)});for(let index=start;index<end;index++)tiles.push({assetId,offset:(index-start)*TILE_BYTES,length:TILE_BYTES as 768,sha256:tileDigests[index]});}
  const audioEnabled=(options.audio??true)&&capabilities.audio&&capabilities.audioFormats.includes('wav-pcm16'),sounds:CompiledWidgetArtifact['sounds']=[];
  if((s.sounds?.length??0)>0&&!audioEnabled)fallbacks.push({feature:'audio',reason:options.audio===false?'Audio disabled by authoring preference; cues omitted.':'Target does not advertise PCM16 audio support; cues omitted.'});
  if(audioEnabled)for(const sound of s.sounds??[]){const asset=s.assets!.find(asset=>asset.id===sound.assetId)!,bytes=decodeWidgetBase64(asset.base64),digest=await widgetDigest(bytes),assetId='audio-'+digest.slice(0,32);if(!descriptors.some(asset=>asset.id===assetId)){descriptors.push({id:assetId,sha256:digest,sizeBytes:bytes.length,mime:'audio/wav'});payloads.push({assetId,base64:asset.base64});}sounds.push({id:sound.id,cue:sound.cue,assetId,cooldownMs:sound.cooldownMs??0,stopOnExit:sound.stopOnExit??true,volume:sound.volume??1});}
  const dictionaryBytes=descriptors.filter(asset=>asset.mime!=='audio/wav').reduce((sum,asset)=>sum+asset.sizeBytes,0),mediaBytes=descriptors.filter(asset=>asset.mime==='audio/wav').reduce((sum,asset)=>sum+asset.sizeBytes,0),transferBytes=dictionaryBytes+mediaBytes;
  const scaleX=preflight.width/s.size.width,scaleY=preflight.height/s.size.height,audioWorkingBytes=Math.max(0,...descriptors.filter(asset=>asset.mime==='audio/wav').map(asset=>asset.sizeBytes))*16;
  const artifact:CompiledWidgetArtifact={format:RASTER_FORMAT,compiler:'osg-widget-compiler-1',rasterizer:'resvg-wasm-2.6.2',id:s.id,sourceHash,parameterHash,behaviorHash,capabilityHash,width:preflight.width,height:preflight.height,tileSize:16,pixelFormat:'rgb565le-a8',behavior:s.behavior,inputs:s.inputs,outputs:s.outputs,hitRegions:s.hitRegions.map(hit=>{const x=Math.floor(hit.x*scaleX),y=Math.floor(hit.y*scaleY);return {...hit,x,y,width:Math.max(1,Math.min(preflight.width-x,Math.round(hit.width*scaleX))),height:Math.max(1,Math.min(preflight.height-y,Math.round(hit.height*scaleY)))};}),domains:preflight.domains,frames,tiles,assets:descriptors,motions,sounds,fallbacks,resources:{frameCount:frames.length,uniqueTiles:tiles.length,dictionaryBytes,frameMapBytes:frames.length*Math.ceil(preflight.width/TILE_SIZE)*Math.ceil(preflight.height/TILE_SIZE)*4,mediaBytes,transferBytes,manifestBytes:0,residentBytes:0,decodeScratchBytes:TILE_BYTES,activeCandidateBytes:0,worstChangedPixels:preflight.width*preflight.height,rawFrameBytes:frames.length*preflight.width*preflight.height*3,audioWorkingBytes}};
  for(let attempt=0;attempt<12;attempt++){
    artifact.resources.residentBytes=transferBytes*3+dictionaryBytes+artifact.resources.manifestBytes*4+artifact.resources.frameMapBytes*2+preflight.width*preflight.height*8+TILE_BYTES+audioWorkingBytes;
    artifact.resources.activeCandidateBytes=artifact.resources.residentBytes*2;
    const bytes=new TextEncoder().encode(canonicalWidget(artifact)).length;if(bytes===artifact.resources.manifestBytes)break;artifact.resources.manifestBytes=bytes;
  }
  abort(options.signal);const admitted=validateCompiledArtifact(artifact,capabilities);assertSource(admitted.ok,admitted.ok?'':admitted.error);
  const bundle=freeze({artifact,assets:payloads,artifactHash:await widgetDigest(new TextEncoder().encode(canonicalWidget(artifact)))});abort(options.signal);cache.set(key,bundle);while(cache.size>8)cache.delete(cache.keys().next().value!);options.onProgress?.({completed:frames.length,total:frames.length,phase:'complete'});return bundle;
}
/** One editor owns this coordinator: every edit cancels and fences old results. */
export function createWidgetCompileCoordinator(){
  let revision=0,controller:AbortController|undefined,current:CompiledWidgetBundle|undefined;
  return {
    get revision(){return revision;},get current(){return current;},
    invalidate(){revision++;controller?.abort();current=undefined;},
    async compile(source:WidgetSource,options:CompileOptions={}){controller?.abort();const active=new AbortController();controller=active;const captured=++revision;current=undefined;const externalAbort=()=>active.abort();options.signal?.addEventListener('abort',externalAbort,{once:true});if(options.signal?.aborted)active.abort();try{const result=await compileWidget(source,{...options,signal:active.signal});if(captured!==revision)throw new DOMException('Stale widget compilation','AbortError');current=result;return result;}finally{options.signal?.removeEventListener('abort',externalAbort);}},
  };
}
