import { canonicalWidget, decodeWidgetBase64, widgetDigest, ARTIFACT_LIMITS, type CompiledAssetDescriptor, type CompiledAssetPayload } from '../../../packages/widget-runtime/src/artifact.ts';
import { compiledPanelCapabilityProjection, validateCompiledPanelDocument, verifyCompiledPanelPublication, type CompiledPanelPublication } from '../../../packages/widget-runtime/src/panel.ts';

export const COMPILED_CACHE_LIMITS = Object.freeze({ assets: 128, bytes: 24 * 1024 * 1024, pending: 16 });
export interface CompiledCacheStore { read(): Promise<unknown>; write(value: string): Promise<void> }
interface CachedAsset { descriptor: CompiledAssetDescriptor; base64: string; usedAt: number }
export interface CompiledDelivery { canonicalDocument: string; assets: CompiledAssetPayload[]; publicationHash: string; capabilityFingerprint?: string; document?: unknown }
const size = (text: string) => new TextEncoder().encode(text).length;
const plain = (value: unknown): Record<string, any> => { if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid compiled cache object.'); return value as Record<string, any>; };
async function verifyAsset(descriptor: CompiledAssetDescriptor, base64: string): Promise<void> {
  if (!descriptor || typeof descriptor.id !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(descriptor.id) || !/^[a-f0-9]{64}$/.test(descriptor.sha256) || !Number.isSafeInteger(descriptor.sizeBytes) || descriptor.sizeBytes < 1 || descriptor.sizeBytes > ARTIFACT_LIMITS.assetBytes || !['application/vnd.opensimgear.raster-tiles','audio/wav'].includes(descriptor.mime) || Object.keys(descriptor).some(key=>!['id','sha256','sizeBytes','mime'].includes(key))) throw new Error('Invalid compiled cache descriptor.');
  const bytes=decodeWidgetBase64(base64);
  if(bytes.length!==descriptor.sizeBytes||await widgetDigest(bytes)!==descriptor.sha256)throw new Error('Cached compiled asset failed its digest check.');
}

/** Public immutable pixels/media only. A cache hit confers no pairing or input authority. */
export function createCompiledAssetCache(store: CompiledCacheStore, options: { maxBytes?: number; maxAssets?: number; clock?: () => number } = {}) {
  const maximumBytes=options.maxBytes??COMPILED_CACHE_LIMITS.bytes, maximumAssets=options.maxAssets??COMPILED_CACHE_LIMITS.assets,clock=options.clock??Date.now;
  let rows:CachedAsset[]|undefined,tail:Promise<unknown>=Promise.resolve(),pending=0;
  const serialize=(assets:CachedAsset[])=>JSON.stringify({version:1,assets});
  async function load(){
    if(rows)return;
    const stored=await store.read();if(stored===undefined||stored===null){rows=[];return;}
    if(typeof stored!=='string'||size(stored)>maximumBytes)throw new Error('Compiled cache exceeds its storage budget.');
    const root=plain(JSON.parse(stored));
    if(root.version!==1||Object.keys(root).some(key=>!['version','assets'].includes(key))||!Array.isArray(root.assets)||root.assets.length>maximumAssets)throw new Error('Unsupported compiled cache.');
    const checked:CachedAsset[]=[],digests=new Set<string>();
    for(const value of root.assets){const row=plain(value);if(Object.keys(row).some(key=>!['descriptor','base64','usedAt'].includes(key))||!Number.isSafeInteger(row.usedAt)||row.usedAt<0)throw new Error('Invalid compiled cache metadata.');await verifyAsset(row.descriptor,row.base64);if(digests.has(row.descriptor.sha256))throw new Error('Duplicate compiled cache digest.');digests.add(row.descriptor.sha256);checked.push(structuredClone(row) as CachedAsset);}
    rows=checked;
  }
  function ordered<T>(action:()=>Promise<T>):Promise<T>{if(pending>=COMPILED_CACHE_LIMITS.pending)return Promise.reject(new Error('Compiled cache is busy.'));pending++;const result=tail.then(action).finally(()=>pending--);tail=result.catch(()=>undefined);return result;}
  async function persist(next:CachedAsset[]){const encoded=serialize(next);if(size(encoded)>maximumBytes)throw new Error('Compiled cache is full.');await store.write(encoded);rows=next;}
  return {
    haveAssets:()=>ordered(async()=>{await load();return rows!.slice(0,64).map(row=>row.descriptor.sha256);}),
    clear:()=>ordered(async()=>persist([])),
    verifyDelivery:(payload:CompiledDelivery)=>ordered(async()=>{
      await load();
      if(typeof payload.canonicalDocument!=='string'||size(payload.canonicalDocument)>ARTIFACT_LIMITS.manifestBytes)throw new Error('Compiled publication manifest exceeds its budget.');
      const document=JSON.parse(payload.canonicalDocument);
      if(canonicalWidget(document)!==payload.canonicalDocument||payload.document!==undefined&&canonicalWidget(payload.document)!==payload.canonicalDocument)throw new Error('Compiled publication differs from its canonical bytes.');
      const checked=validateCompiledPanelDocument(document);if(!checked.ok)throw new Error(checked.error);
      if(!Array.isArray(payload.assets)||payload.assets.length>ARTIFACT_LIMITS.assets)throw new Error('Invalid compiled delivery assets.');
      const incoming=new Map<string,CompiledAssetPayload>();
      for(const asset of payload.assets){plain(asset);if(Object.keys(asset).some(key=>!['assetId','base64'].includes(key))||incoming.has(asset.assetId)||!document.ui.assets.some((descriptor:CompiledAssetDescriptor)=>descriptor.id===asset.assetId))throw new Error('Unknown or duplicate compiled delivery asset.');incoming.set(asset.assetId,asset);}
      const assets:CompiledAssetPayload[]=document.ui.assets.map((descriptor:CompiledAssetDescriptor)=>{const received=incoming.get(descriptor.id);if(received)return received;const cached=rows!.find(row=>row.descriptor.sha256===descriptor.sha256&&row.descriptor.sizeBytes===descriptor.sizeBytes);if(!cached)throw new Error('Compiled delivery is missing an uncached asset.');return {assetId:descriptor.id,base64:cached.base64};});
      const fingerprint='sha256:'+await widgetDigest(new TextEncoder().encode(canonicalWidget(compiledPanelCapabilityProjection(document))));
      const publication=await verifyCompiledPanelPublication({document,assets,publicationHash:payload.publicationHash,capabilityFingerprint:payload.capabilityFingerprint??fingerprint});
      return publication;
    }),
    save:(publication:CompiledPanelPublication)=>ordered(async()=>{
      await load();const checked=await verifyCompiledPanelPublication(publication),at=clock();
      if(!Number.isSafeInteger(at)||at<0)throw new Error('Invalid compiled cache clock.');
      const added=[...new Map(checked.document.ui.assets.map(descriptor=>[descriptor.sha256,{descriptor:structuredClone(descriptor),base64:checked.assets.find(payload=>payload.assetId===descriptor.id)!.base64,usedAt:at}])).values()];
      const current=new Set(added.map(row=>row.descriptor.sha256));
      const next=[...added,...rows!.filter(row=>!current.has(row.descriptor.sha256)).sort((a,b)=>b.usedAt-a.usedAt)].slice(0,maximumAssets);
      while(next.length&&size(serialize(next))>maximumBytes)next.pop();
      await persist(next);
    }),
  };
}

export function browserCompiledCacheStore(factory:IDBFactory|undefined=globalThis.indexedDB):CompiledCacheStore {
  let opened:Promise<IDBDatabase>|undefined;
  function open(){return opened??=new Promise<IDBDatabase>((resolve,reject)=>{
    if(!factory){reject(new Error('Durable compiled asset storage is unavailable.'));return;}
    const request=factory.open('opensimgear-browser-compiled-assets',1);
    request.onupgradeneeded=()=>{if(!request.result.objectStoreNames.contains('assets'))request.result.createObjectStore('assets');};
    request.onerror=()=>reject(new Error('Cannot open compiled asset cache.'));request.onblocked=()=>reject(new Error('Compiled asset cache is blocked.'));
    request.onsuccess=()=>{request.result.onversionchange=()=>request.result.close();resolve(request.result);};
  });}
  async function transaction(mode:IDBTransactionMode,value?:string):Promise<unknown>{const db=await open();return new Promise((resolve,reject)=>{const tx=db.transaction('assets',mode),store=tx.objectStore('assets'),request=mode==='readonly'?store.get('cache'):store.put(value,'cache');let result:unknown;request.onsuccess=()=>result=request.result;tx.oncomplete=()=>resolve(result);tx.onerror=tx.onabort=()=>reject(new Error('Compiled asset cache commit failed.'));});}
  return {read:()=>transaction('readonly'),write:async value=>{await transaction('readwrite',value);}};
}
