import { validateBehaviorDefinition, type BehaviorDefinition, type Scalar } from './behavior.ts';

export const RASTER_FORMAT = 'osg-raster-widget-1' as const;
export const TILE_SIZE = 16;
export const TILE_BYTES = 768;
export const ARTIFACT_LIMITS = Object.freeze({ dimension: 2048, frames: 4096, tiles: 32768, assets: 64, assetBytes: 2 * 1024 * 1024, totalBytes: 8 * 1024 * 1024, manifestBytes: 1024 * 1024 });
export interface WidgetCapabilities {
  version: 1; id: string; rasterFormats: ['rgb565le-a8'];
  maxWidth: number; maxHeight: number; maxFrames: number;
  maxAssetBytes: number; maxTotalBytes: number; maxResidentBytes: number;
  motion: boolean; audio: boolean; audioFormats: ('wav-pcm16')[];
}
export const BROWSER_WIDGET_CAPABILITIES: WidgetCapabilities = Object.freeze({ version: 1, id: 'browser-raster-v1', rasterFormats: ['rgb565le-a8'] as ['rgb565le-a8'], maxWidth: 2048, maxHeight: 2048, maxFrames: 4096, maxAssetBytes: 2 * 1024 * 1024, maxTotalBytes: 8 * 1024 * 1024, maxResidentBytes: 64 * 1024 * 1024, motion: true, audio: true, audioFormats: ['wav-pcm16'] as ['wav-pcm16'] });
export interface CompiledAssetDescriptor { id: string; sha256: string; sizeBytes: number; mime: 'application/vnd.opensimgear.raster-tiles' | 'audio/wav' }
export interface CompiledAssetPayload { assetId: string; base64: string }
export interface CompiledTile { assetId: string; offset: number; length: 768; sha256: string }
export interface CompiledSelector { values: Record<string, Scalar>; regions: Record<string, string> }
export interface CompiledFrame { id: string; tiles: number[]; selector?: CompiledSelector }
export interface CompiledDomain { signal: string; values: Scalar[]; numeric: boolean; maxErrorPixels: number; smoothingMs: number }
export interface CompiledPort { id: string; label: string; type: 'boolean' | 'number' | 'enum'; signal: string; min?: number; max?: number; values?: string[] }
export interface CompiledHitRegion { id: string; x: number; y: number; width: number; height: number; events: Partial<Record<'press' | 'release' | 'activate' | 'cancel', string>> }
export interface CompiledMotion { id: string; fromFrameId: string; toFrameId: string; frames: string[]; durationMs: number; interruption: 'reverse' | 'restart' | 'snap' }
export interface CompiledSound { id: string; cue: string; assetId: string; cooldownMs: number; stopOnExit: boolean; volume: number }
export interface ResourceReport {
  frameCount: number; uniqueTiles: number; dictionaryBytes: number; frameMapBytes: number;
  mediaBytes: number; transferBytes: number; manifestBytes: number;
  residentBytes: number; decodeScratchBytes: number; activeCandidateBytes: number;
  worstChangedPixels: number; rawFrameBytes: number; audioWorkingBytes: number;
}
export interface CompiledWidgetArtifact {
  format: typeof RASTER_FORMAT; compiler: string; rasterizer: string;
  id: string; sourceHash: string; parameterHash: string; behaviorHash: string; capabilityHash: string;
  width: number; height: number; tileSize: 16; pixelFormat: 'rgb565le-a8';
  behavior: BehaviorDefinition; inputs: CompiledPort[]; outputs: CompiledPort[];
  hitRegions: CompiledHitRegion[]; domains: CompiledDomain[];
  frames: CompiledFrame[]; tiles: CompiledTile[]; assets: CompiledAssetDescriptor[];
  motions: CompiledMotion[]; sounds: CompiledSound[];
  fallbacks: { feature: 'motion' | 'audio'; reason: string }[];
  resources: ResourceReport;
}
export interface CompiledWidgetBundle { artifact: CompiledWidgetArtifact; assets: CompiledAssetPayload[]; artifactHash: string }
export type ArtifactValidation<T> = { ok: true; value: T } | { ok: false; error: string };
function requireThat(test: unknown, message: string): asserts test { if (!test) throw new Error(message); }
function object(value: unknown, required: string[], optional: string[] = []): Record<string, any> {
  requireThat(value && typeof value === 'object' && !Array.isArray(value) && Object.getPrototypeOf(value) === Object.prototype, 'Expected plain artifact object');
  const v = value as Record<string, any>;
  requireThat(required.every(key => Object.hasOwn(v, key)) && Object.keys(v).every(key => required.includes(key) || optional.includes(key)), 'Unexpected or missing artifact field'); return v;
}
function integer(value: unknown, min: number, max: number): asserts value is number { requireThat(Number.isSafeInteger(value) && Number(value) >= min && Number(value) <= max, 'Artifact integer outside bounds'); }
function list(value: unknown, max: number): any[] { requireThat(Array.isArray(value) && value.length <= max, 'Artifact list outside bounds'); return value; }
function stable(value: unknown): asserts value is string { requireThat(typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value), 'Invalid artifact ID'); }
function hash(value: unknown): void { requireThat(typeof value === 'string' && /^[a-f0-9]{64}$/.test(value), 'Invalid artifact digest'); }
export function canonicalWidget(value: unknown): string {
  if (value === null || typeof value !== 'object') { requireThat(typeof value !== 'number' || Number.isFinite(value), 'Nonfinite canonical value'); return JSON.stringify(value); }
  if (Array.isArray(value)) return '[' + value.map(canonicalWidget).join(',') + ']';
  return '{' + Object.keys(value).sort().map(key => JSON.stringify(key) + ':' + canonicalWidget((value as Record<string, unknown>)[key])).join(',') + '}';
}
export async function widgetDigest(bytes: Uint8Array): Promise<string> { return Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', Uint8Array.from(bytes).buffer))).map(byte => byte.toString(16).padStart(2, '0')).join(''); }
export function encodeWidgetBase64(bytes: Uint8Array): string { let binary = ''; for (let offset = 0; offset < bytes.length; offset += 8192) binary += String.fromCharCode(...bytes.subarray(offset, offset + 8192)); return btoa(binary); }
export function decodeWidgetBase64(value: string): Uint8Array { requireThat(typeof value === 'string' && value.length <= Math.ceil(ARTIFACT_LIMITS.assetBytes / 3) * 4 && /^[A-Za-z0-9+/]*={0,2}$/.test(value) && value.length % 4 === 0, 'Invalid compiled asset base64'); const bytes = Uint8Array.from(atob(value), character => character.charCodeAt(0)); requireThat(encodeWidgetBase64(bytes) === value, 'Noncanonical compiled asset base64'); return bytes; }
export function validateWidgetCapabilities(input: unknown): ArtifactValidation<WidgetCapabilities> {
  try {
    const c = object(input, ['version','id','rasterFormats','maxWidth','maxHeight','maxFrames','maxAssetBytes','maxTotalBytes','maxResidentBytes','motion','audio','audioFormats']);
    requireThat(c.version === 1, 'Unsupported widget capability version'); stable(c.id);
    requireThat(Array.isArray(c.rasterFormats) && canonicalWidget(c.rasterFormats) === '["rgb565le-a8"]', 'Unsupported raster capability');
    integer(c.maxWidth, 1, ARTIFACT_LIMITS.dimension); integer(c.maxHeight, 1, ARTIFACT_LIMITS.dimension); integer(c.maxFrames, 1, ARTIFACT_LIMITS.frames);
    integer(c.maxAssetBytes, TILE_BYTES, ARTIFACT_LIMITS.assetBytes); integer(c.maxTotalBytes, TILE_BYTES, ARTIFACT_LIMITS.totalBytes); integer(c.maxResidentBytes, TILE_BYTES, 256 * 1024 * 1024);
    requireThat(typeof c.motion === 'boolean' && typeof c.audio === 'boolean', 'Invalid media capabilities'); requireThat(Array.isArray(c.audioFormats) && c.audioFormats.length <= 1 && c.audioFormats.every((format: unknown) => format === 'wav-pcm16'), 'Unsupported audio capability');
    return { ok: true, value: c as WidgetCapabilities };
  } catch (e) { return { ok: false, error: e instanceof Error ? e.message : 'Invalid widget capabilities' }; }
}
export function validateCompiledArtifact(input: unknown, capabilities: WidgetCapabilities = BROWSER_WIDGET_CAPABILITIES): ArtifactValidation<CompiledWidgetArtifact> {
  try {
    const capability = validateWidgetCapabilities(capabilities); requireThat(capability.ok, capability.ok ? '' : capability.error);
    const a = object(input, ['format','compiler','rasterizer','id','sourceHash','parameterHash','behaviorHash','capabilityHash','width','height','tileSize','pixelFormat','behavior','inputs','outputs','hitRegions','domains','frames','tiles','assets','motions','sounds','fallbacks','resources']);
    requireThat(a.format === RASTER_FORMAT && a.compiler === 'osg-widget-compiler-1' && a.rasterizer === 'resvg-wasm-2.6.2', 'Unsupported compiled widget format/compiler'); stable(a.id);
    for (const field of ['sourceHash','parameterHash','behaviorHash','capabilityHash']) hash(a[field]);
    integer(a.width, 1, capabilities.maxWidth); integer(a.height, 1, capabilities.maxHeight); requireThat(a.tileSize === TILE_SIZE && a.pixelFormat === 'rgb565le-a8', 'Unsupported pixel encoding');
    const behavior = validateBehaviorDefinition(a.behavior); requireThat(behavior.ok, behavior.ok ? '' : behavior.error);
    const signals = new Map<string,any>(a.behavior.signals.map((signal: any) => [signal.id, signal]));
    for (const ports of [a.inputs,a.outputs]) { const ids = new Set(); for (const port of list(ports, 16)) {
      object(port,['id','label','type','signal'],['min','max','values']); stable(port.id); requireThat(!ids.has(port.id), 'Duplicate compiled port'); ids.add(port.id);
      const signal=signals.get(port.signal);requireThat(typeof port.label === 'string' && port.label.length <= 120 && signal && port.type===signal.type, 'Invalid compiled port');
      if(signal.type==='number')requireThat(port.min===signal.min && port.max===signal.max && port.values===undefined,'Compiled port numeric domain mismatch');
      else if(signal.type==='enum')requireThat(canonicalWidget(port.values)===canonicalWidget(signal.values) && port.min===undefined && port.max===undefined,'Compiled port enum domain mismatch');
      else requireThat(port.min===undefined && port.max===undefined && port.values===undefined,'Unexpected compiled port domain');
      if(ports===a.inputs){const output=a.behavior.outputs?.find((output:any)=>output.id===port.id);requireThat(output && output.type===signal.type && output.neutral===signal.initial,'Compiled input must have a matching declared emit output and neutral');if(output.type==='number')requireThat(output.min===port.min && output.max===port.max,'Compiled emit numeric domain mismatch');if(output.type==='enum')requireThat(canonicalWidget(output.values)===canonicalWidget(port.values),'Compiled emit enum domain mismatch');}
    } }
    const events = new Set(a.behavior.events.map((event: any) => event.id));
    const hits = new Set(); for (const region of list(a.hitRegions, 32)) {
      object(region,['id','x','y','width','height','events']); stable(region.id); requireThat(!hits.has(region.id), 'Duplicate hit region'); hits.add(region.id);
      integer(region.x, 0, a.width); integer(region.y, 0, a.height); integer(region.width, 1, a.width); integer(region.height, 1, a.height); requireThat(region.x + region.width <= a.width && region.y + region.height <= a.height, 'Hit region outside surface');
      const mappings=object(region.events,[],['press','release','activate','cancel']); requireThat(Object.values(mappings).every(event => events.has(event)), 'Unknown hit event');
    }
    const domainSignals = new Set(); for (const domain of list(a.domains, 16)) {
      object(domain,['signal','values','numeric','maxErrorPixels','smoothingMs']); requireThat(signals.has(domain.signal) && !domainSignals.has(domain.signal), 'Invalid visual domain signal'); domainSignals.add(domain.signal);integer(domain.smoothingMs,0,60000);requireThat(domain.numeric || domain.smoothingMs===0,'Only numeric visual domains support smoothing');requireThat(capabilities.motion || domain.smoothingMs===0,'Target does not advertise numeric smoothing');
      const signal=signals.get(domain.signal),values=list(domain.values,4096);requireThat(values.length>0 && new Set(values.map(canonicalWidget)).size===values.length,'Invalid visual domain values');
      requireThat(domain.numeric===(signal.type==='number') && values.every(value=>signal.type==='boolean'?typeof value==='boolean':signal.type==='enum'?signal.values.includes(value):typeof value==='number'&&Number.isFinite(value)&&value>=signal.min&&value<=signal.max),'Visual domain incompatible with behavior signal');
      requireThat(typeof domain.maxErrorPixels==='number' && Number.isFinite(domain.maxErrorPixels) && domain.maxErrorPixels>=0,'Invalid visual domain accuracy');
      if(domain.numeric)requireThat(values.length>=2 && values[0]===signal.min && values.at(-1)===signal.max && values.every((value,index)=>index===0||value>values[index-1]),'Numeric samples must increase and cover both signal bounds');
      else requireThat(signal.type==='boolean'?values.length===2&&values.includes(false)&&values.includes(true):values.length===signal.values.length&&signal.values.every((value:string)=>values.includes(value)),'Discrete visual domain must cover every signal value');
    }
    const assets = list(a.assets, ARTIFACT_LIMITS.assets), assetMap = new Map<string, any>(); let total = 0, dictionary = 0, media = 0;
    for (const asset of assets) { object(asset,['id','sha256','sizeBytes','mime']); stable(asset.id); hash(asset.sha256); integer(asset.sizeBytes,1,Math.min(ARTIFACT_LIMITS.assetBytes,capabilities.maxAssetBytes)); requireThat(!assetMap.has(asset.id), 'Duplicate compiled asset'); assetMap.set(asset.id,asset); requireThat(['application/vnd.opensimgear.raster-tiles','audio/wav'].includes(asset.mime), 'Unsupported compiled media'); requireThat(asset.id===(asset.mime==='audio/wav'?'audio-':'pixels-')+asset.sha256.slice(0,32),'Compiled asset ID must identify its content'); total += asset.sizeBytes; if(asset.mime==='audio/wav')media+=asset.sizeBytes; else {requireThat(asset.sizeBytes%TILE_BYTES===0,'Pixel dictionary size must align to tiles');dictionary+=asset.sizeBytes;} }
    requireThat(total <= capabilities.maxTotalBytes, 'Compiled assets exceed target byte budget');
    const tiles = list(a.tiles, ARTIFACT_LIMITS.tiles); requireThat(tiles.length > 0, 'Compiled widget needs pixel tiles');
    const tileOffsets=new Set<string>();for (const tile of tiles) { object(tile,['assetId','offset','length','sha256']); hash(tile.sha256); integer(tile.offset,0,ARTIFACT_LIMITS.assetBytes); requireThat(tile.length === TILE_BYTES && tile.offset % TILE_BYTES === 0, 'Invalid tile size/alignment'); const asset=assetMap.get(tile.assetId); requireThat(asset?.mime === 'application/vnd.opensimgear.raster-tiles' && tile.offset + tile.length <= asset.sizeBytes, 'Tile outside pixel dictionary');const key=tile.assetId+':'+tile.offset;requireThat(!tileOffsets.has(key),'Duplicate tile storage offset');tileOffsets.add(key); }
    requireThat(tiles.length*TILE_BYTES===dictionary,'Pixel dictionary contains unreferenced bytes');
    const frames=list(a.frames,capabilities.maxFrames), ids=new Set<string>(), cells=Math.ceil(a.width/TILE_SIZE)*Math.ceil(a.height/TILE_SIZE); requireThat(frames.length>0,'Compiled widget needs frames');
    for (const frame of frames) { object(frame,['id','tiles'],['selector']); stable(frame.id); requireThat(!ids.has(frame.id),'Duplicate compiled frame'); ids.add(frame.id); requireThat(list(frame.tiles,cells).length===cells,'Incorrect frame tile count'); for(const index of frame.tiles)integer(index,0,tiles.length-1); if(frame.selector!==undefined){object(frame.selector,['values','regions']); const values=object(frame.selector.values,[],a.domains.map((d:any)=>d.signal)); for(const domain of a.domains)requireThat(domain.values.some((value:Scalar)=>value===values[domain.signal]),'Frame selector outside visual domain'); object(frame.selector.regions,a.behavior.regions.map((r:any)=>r.id)); for(const region of a.behavior.regions)requireThat(region.states.includes(frame.selector.regions[region.id]),'Frame selector outside region'); } }
    const selectors=frames.filter(frame=>frame.selector!==undefined).map(frame=>canonicalWidget(frame.selector)),expectedSelectors=a.domains.reduce((count:number,domain:any)=>count*domain.values.length,1)*a.behavior.regions.reduce((count:number,region:any)=>count*region.states.length,1);
    requireThat(expectedSelectors<=capabilities.maxFrames && selectors.length===expectedSelectors && new Set(selectors).size===selectors.length,'Static presentation selector coverage is incomplete or duplicated');
    requireThat(new Set(frames.flatMap(frame=>frame.tiles)).size===tiles.length,'Compiled dictionary contains unused tiles');
    const motionIds=new Set();for(const motion of list(a.motions,128)){object(motion,['id','fromFrameId','toFrameId','frames','durationMs','interruption']);stable(motion.id);requireThat(!motionIds.has(motion.id),'Duplicate compiled motion');motionIds.add(motion.id);requireThat(capabilities.motion && ids.has(motion.fromFrameId) && ids.has(motion.toFrameId),'Unsupported or invalid compiled motion');integer(motion.durationMs,1,60000);requireThat(list(motion.frames,600).length>=2 && motion.frames.every((id:string)=>ids.has(id)) && motion.frames[0]===motion.fromFrameId && motion.frames.at(-1)===motion.toFrameId && ['reverse','restart','snap'].includes(motion.interruption),'Invalid compiled motion sequence');}
    const soundIds=new Set();for(const sound of list(a.sounds,32)){object(sound,['id','cue','assetId','cooldownMs','stopOnExit','volume']);stable(sound.id);stable(sound.cue);requireThat(!soundIds.has(sound.id),'Duplicate compiled sound');soundIds.add(sound.id);requireThat(capabilities.audio && capabilities.audioFormats.includes('wav-pcm16') && assetMap.get(sound.assetId)?.mime==='audio/wav','Unsupported or invalid compiled audio');integer(sound.cooldownMs,0,60000);requireThat(typeof sound.stopOnExit==='boolean' && typeof sound.volume==='number' && sound.volume>=0 && sound.volume<=1,'Invalid compiled cue');}
    const usedAssets=new Set([...tiles.map(tile=>tile.assetId),...a.sounds.map((sound:any)=>sound.assetId)]);requireThat(usedAssets.size===assetMap.size && [...assetMap.keys()].every(id=>usedAssets.has(id)),'Compiled asset closure contains unused assets');
    for(const fallback of list(a.fallbacks,2)){object(fallback,['feature','reason']);requireThat(['motion','audio'].includes(fallback.feature) && typeof fallback.reason==='string' && fallback.reason.length<=240,'Invalid fallback report');}
    const resources=object(a.resources,['frameCount','uniqueTiles','dictionaryBytes','frameMapBytes','mediaBytes','transferBytes','manifestBytes','residentBytes','decodeScratchBytes','activeCandidateBytes','worstChangedPixels','rawFrameBytes','audioWorkingBytes']); for(const value of Object.values(resources))integer(value,0,1024*1024*1024);
    const audioWorking=Math.max(0,...assets.filter(asset=>asset.mime==='audio/wav').map(asset=>asset.sizeBytes))*16;
    requireThat(resources.frameCount===frames.length && resources.uniqueTiles===tiles.length && resources.dictionaryBytes===dictionary && resources.mediaBytes===media && resources.transferBytes===total && resources.frameMapBytes===frames.length*cells*4 && resources.decodeScratchBytes===TILE_BYTES && resources.rawFrameBytes===frames.length*a.width*a.height*3 && resources.activeCandidateBytes===resources.residentBytes*2 && resources.worstChangedPixels===a.width*a.height && resources.audioWorkingBytes===audioWorking && resources.residentBytes===total*3+dictionary+resources.manifestBytes*4+resources.frameMapBytes*2+a.width*a.height*8+TILE_BYTES+audioWorking,'Incorrect compiled resource report');
    requireThat(resources.activeCandidateBytes<=capabilities.maxResidentBytes,'Compiled active and candidate widgets exceed resident memory budget');
    const manifestBytes=new TextEncoder().encode(canonicalWidget(a)).length; requireThat(manifestBytes===resources.manifestBytes && manifestBytes<=ARTIFACT_LIMITS.manifestBytes,'Compiled manifest exceeds budget or has incorrect size');
    return {ok:true,value:a as CompiledWidgetArtifact};
  }catch(e){return {ok:false,error:e instanceof Error?e.message:'Invalid compiled artifact'};}
}
export async function verifyCompiledBundle(input: unknown, capabilities: WidgetCapabilities = BROWSER_WIDGET_CAPABILITIES): Promise<CompiledWidgetBundle> {
  const bundle=object(input,['artifact','assets','artifactHash']);hash(bundle.artifactHash);const checked=validateCompiledArtifact(bundle.artifact,capabilities);requireThat(checked.ok,checked.ok?'':checked.error);
  requireThat(await widgetDigest(new TextEncoder().encode(canonicalWidget(bundle.artifact)))===bundle.artifactHash,'Compiled artifact digest mismatch');
  requireThat(await widgetDigest(new TextEncoder().encode(canonicalWidget(bundle.artifact.behavior)))===bundle.artifact.behaviorHash,'Compiled behavior digest mismatch');
  const payloads=list(bundle.assets,ARTIFACT_LIMITS.assets);requireThat(payloads.length===bundle.artifact.assets.length,'Incomplete compiled asset closure');const bytes=new Map<string,Uint8Array>();
  for(const payload of payloads){object(payload,['assetId','base64']);requireThat(!bytes.has(payload.assetId),'Duplicate compiled payload');const descriptor=bundle.artifact.assets.find((asset:CompiledAssetDescriptor)=>asset.id===payload.assetId);requireThat(descriptor,'Unknown compiled payload');const data=decodeWidgetBase64(payload.base64);requireThat(data.length===descriptor.sizeBytes && await widgetDigest(data)===descriptor.sha256,'Compiled asset digest/size mismatch');bytes.set(payload.assetId,data);}
  for(const tile of bundle.artifact.tiles)requireThat(await widgetDigest(bytes.get(tile.assetId)!.subarray(tile.offset,tile.offset+tile.length))===tile.sha256,'Pixel tile digest mismatch');
  for(const asset of bundle.artifact.assets)if(asset.mime==='audio/wav')validatePcmWave(bytes.get(asset.id)!);
  return bundle as CompiledWidgetBundle;
}
/** Only already validated PCM16 little-endian WAV is deployable in this version. */
export function validatePcmWave(bytes: Uint8Array): { channels:number; sampleRate:number; durationMs:number } {
  requireThat(bytes.length>=44 && bytes.length<=ARTIFACT_LIMITS.assetBytes,'Invalid WAV size');const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),tag=(offset:number)=>String.fromCharCode(...bytes.subarray(offset,offset+4));
  requireThat(tag(0)==='RIFF' && tag(8)==='WAVE' && view.getUint32(4,true)+8===bytes.length,'Invalid WAV container');let cursor=12,channels=0,sampleRate=0,dataBytes=0;
  while(cursor+8<=bytes.length){const length=view.getUint32(cursor+4,true),start=cursor+8;requireThat(start+length<=bytes.length,'Truncated WAV chunk');if(tag(cursor)==='fmt '){requireThat(!channels && length>=16 && view.getUint16(start,true)===1 && view.getUint16(start+14,true)===16,'Only PCM16 WAV is supported');channels=view.getUint16(start+2,true);sampleRate=view.getUint32(start+4,true);requireThat((channels===1||channels===2) && sampleRate>=8000 && sampleRate<=48000 && view.getUint16(start+12,true)===channels*2 && view.getUint32(start+8,true)===sampleRate*channels*2,'Invalid WAV sample format');}else if(tag(cursor)==='data'){requireThat(!dataBytes && length>0,'Invalid WAV data');dataBytes=length;}cursor=start+length+(length%2);}
  requireThat(cursor===bytes.length && channels>0 && dataBytes>0 && dataBytes%(channels*2)===0,'Incomplete WAV');const durationMs=dataBytes/(sampleRate*channels*2)*1000;requireThat(durationMs<=10000,'Sound exceeds ten seconds');return {channels,sampleRate,durationMs};
}
/** Copy finished pixels only; no layer/tree/vector interpretation occurs here. */
export type CompiledPixels=Uint8ClampedArray<ArrayBuffer>;
/** Each decode reuses the same RGBA buffer; its contents change on the next call. */
export function createCompiledPixelDecoder(artifact: CompiledWidgetArtifact, assets: CompiledAssetPayload[]): { decode(frameId:string):CompiledPixels; dispose():void } {
  const dictionaries=new Set(artifact.tiles.map(tile=>tile.assetId));
  const data=new Map(assets.filter(asset=>dictionaries.has(asset.assetId)).map(asset=>[asset.assetId,decodeWidgetBase64(asset.base64)])),frames=new Map(artifact.frames.map(frame=>[frame.id,frame]));let buffer:CompiledPixels|undefined=new Uint8ClampedArray(artifact.width*artifact.height*4);
  return {dispose(){buffer=undefined;data.clear();frames.clear();},decode(frameId:string){
    const pixels=buffer;requireThat(pixels,'Compiled decoder disposed');const frame=frames.get(frameId);requireThat(frame,'Unknown compiled frame');const columns=Math.ceil(artifact.width/TILE_SIZE);
    frame.tiles.forEach((index,cell)=>{const tile=artifact.tiles[index];requireThat(tile,'Missing compiled tile');const bytes=data.get(tile.assetId);requireThat(bytes && tile.offset+TILE_BYTES<=bytes.length,'Missing pixel dictionary');const left=(cell%columns)*TILE_SIZE,top=Math.floor(cell/columns)*TILE_SIZE;for(let y=0;y<TILE_SIZE && top+y<artifact.height;y++)for(let x=0;x<TILE_SIZE && left+x<artifact.width;x++){const local=y*TILE_SIZE+x,offset=tile.offset+local*2,rgb=bytes[offset]|bytes[offset+1]<<8,target=((top+y)*artifact.width+left+x)*4;pixels[target]=Math.round(((rgb>>11)&31)*255/31);pixels[target+1]=Math.round(((rgb>>5)&63)*255/63);pixels[target+2]=Math.round((rgb&31)*255/31);pixels[target+3]=bytes[tile.offset+512+local];}});return pixels;
  }};
}
export function decodeCompiledFrame(artifact: CompiledWidgetArtifact, assets: CompiledAssetPayload[], frameId: string): CompiledPixels {
  const decoder=createCompiledPixelDecoder(artifact,assets);try{return decoder.decode(frameId);}finally{decoder.dispose();}
}
