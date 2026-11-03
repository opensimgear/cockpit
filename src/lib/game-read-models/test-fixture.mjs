import {createHash,webcrypto} from 'node:crypto';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
import ts from 'typescript';
import {SharedObservations} from '../streams/shared.ts';
const modules=new Map();
export async function load(specifier,parent=import.meta.url){
  const url=new URL(specifier.endsWith('.ts')?specifier:`${specifier}.ts`,parent),key=url.href;
  if(modules.has(key))return modules.get(key);
  if(url.pathname.endsWith('/streams/shared.ts'))return {SharedObservations};
  if(url.pathname.endsWith('/gateway.ts'))return {gatewayRequest:()=>{throw Error('Unexpected native page request');}};
  const code=ts.transpileModule(await readFile(url,'utf8'),{fileName:fileURLToPath(url),compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,inlineSourceMap:true,inlineSources:true}}).outputText;
  const dependencies=new Map();for(const match of code.matchAll(/require\(["']([^"']+)["']\)/g))dependencies.set(match[1],await load(match[1],url));
  const exports={};vm.runInNewContext(code,{exports,structuredClone,TextEncoder,TextDecoder,AbortController,Event,crypto:webcrypto,require:name=>dependencies.get(name)},{filename:fileURLToPath(url)});modules.set(key,exports);return exports;
}
export const {GameReadModels}=await load('./shared');
export const {GamePages,MAX_GAME_BLOB_BYTES,MAX_GAME_LOGICAL_NODES,capturePageReference,assetReference}=await load('./pages');
export const models=await load('./models');
export const imageModule=await load('./image');
export const game=(id='dcs',name='DCS')=>({id,name,installed:true,launchSupported:true});
export const summary=(rows=[game()])=>({catalogue:rows,detectedGames:[],detectionErrors:[],tags:{}});
export const frame=(payload,topic='games.state',changes={})=>({kind:'snapshot',topic,source:'games',session:'broker',generation:'1',revision:'1',sequence:'1',capturedUs:'1',clockId:'games:11111111-1111-4111-8111-111111111111',continuity:'baseline',payload,...changes});
export const change=(revision=2,extras={})=>({kind:'change',continuity:'continuous',revision:String(revision),sequence:String(revision),...extras});
export function harness(getPage){
  const opens=[];const shared=new SharedObservations({open(topic,observer,signal){opens.push({topic,observer,signal,delivery:0});}});const client=new GameReadModels(shared,getPage);
  function send(payload,options={},entry=opens.at(-1)){entry.observer.frame({subscriptionId:entry.topic,delivery:String(++entry.delivery),frame:frame(payload,entry.topic,options)},async()=>{});}
  return {client,opens,send};
}
export function blobs(){
  const values=new Map(),calls=[];
  function add(source,kind){const bytes=new TextEncoder().encode(typeof source==='string'?source:JSON.stringify(source)),id=`sha256:${createHash('sha256').update(bytes).digest('hex')}`;values.set(id,bytes);return{$gamePage:{id,bytes:bytes.length,kind}};}
  async function get(id,offset,signal){calls.push({id,offset,signal});const bytes=values.get(id);if(!bytes)throw Error('Page expired');let end=Math.min(offset+8192,bytes.length);while(end<bytes.length&&(bytes[end]&0xc0)===0x80)end--;return{id,offset,nextOffset:end===bytes.length?null:end,total:bytes.length,chunk:new TextDecoder().decode(bytes.subarray(offset,end))};}
  return {add,get,calls,values};
}
export const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};
export async function until(condition){const deadline=Date.now()+10000;while(Date.now()<deadline){if(condition())return;await new Promise(resolve=>setTimeout(resolve,1));}throw Error('Game read-model resolution did not settle.');}
