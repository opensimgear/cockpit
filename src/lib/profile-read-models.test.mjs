import {createHash,webcrypto} from 'node:crypto';
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
import ts from 'typescript';
import {SharedObservations} from './streams/shared.ts';
// Resolve nested pure modules exactly as the app does; retain original filenames for coverage.
const modules=new Map();
async function load(specifier,parent=import.meta.url){
  const url=new URL(specifier.endsWith('.ts')?specifier:`${specifier}.ts`,parent),key=url.href;
  if(modules.has(key))return modules.get(key);
  if(url.pathname.endsWith('/streams/shared.ts'))return {SharedObservations};
  if(url.pathname.endsWith('/gateway.ts'))return {gatewayRequest:()=>{throw Error('Unexpected native page request');}};
  const code=ts.transpileModule(await readFile(url,'utf8'),{fileName:fileURLToPath(url),compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,inlineSourceMap:true,inlineSources:true}}).outputText;
  const dependencies=new Map();for(const match of code.matchAll(/require\(["']([^"']+)["']\)/g))dependencies.set(match[1],await load(match[1],url));
  const exports={};vm.runInNewContext(code,{exports,structuredClone,TextEncoder,TextDecoder,AbortController,crypto:webcrypto,require:name=>dependencies.get(name)},{filename:fileURLToPath(url)});modules.set(key,exports);return exports;
}
const {ProfileReadModels}=await load('./profile-read-models/shared');
const {captureLibrary,captureRuntime}=await load('./profile-read-models/models');
const {DEFAULT_PROFILE_ID}=await load('./profiles');
const id=DEFAULT_PROFILE_ID, producer='11111111-1111-4111-8111-111111111111';
const summary={id,name:'Default',revision:1,isDefault:true,semanticHash:`sha256:${'1'.repeat(64)}`,gameIds:[]};
const session={selectedProfileId:id,selectionMode:'manual',state:'stopped',appliedSnapshot:null,fault:null};
const library=()=>({profiles:[summary],session,diagnostics:[],readOnly:false,versions:{[id]:'1'}});
const automatic={schemaVersion:1,mode:'manual',manualProfileId:id,rules:[],status:'manual',message:'',conflicts:[],lastAttempt:null,target:null,readOnly:false};
const runtime=()=>({session,diagnostics:[],readOnly:false,profileSession:{state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null},automatic,preferredProfiles:{dcs:id}});
const frame=(payload,topic='profiles.library',changes={})=>({kind:'snapshot',topic,source:'profiles',session:'broker',generation:'1',revision:'1',sequence:'1',capturedUs:'1',clockId:`profiles:${producer}`,continuity:'baseline',payload,...changes});
function harness(getPage){
 const opens=[];const shared=new SharedObservations({open(topic,observer,signal){opens.push({topic,observer,signal,delivery:0});}});const client=new ProfileReadModels(shared,getPage);
 function send(payload,options={},entry=opens.at(-1)){entry.observer.frame({subscriptionId:entry.topic,delivery:String(++entry.delivery),frame:frame(payload,entry.topic,options)},async()=>{});}
 return {client,opens,send};
}
test('profile models validate complete typed replacements, immutable versions and preference metadata',()=>{
 const source=library();const value=captureLibrary(frame(source));source.profiles[0]={...summary,name:'Mutation'};assert.equal(value.profiles[0].name,'Default');assert.equal(Object.isFrozen(value.versions),true);
 assert.equal(captureRuntime(frame(runtime(),'profiles.runtime')).preferredProfiles.dcs,id);
 for(const broken of [{...library(),versions:{}},{...library(),versions:{[id]:1}},{...library(),document:{}},{...library(),session:{...session,state:'invented'}}])assert.throws(()=>captureLibrary(frame(broken)));
 assert.throws(()=>captureLibrary(frame(library(),'profiles.library',{source:'other'})));
 assert.throws(()=>captureLibrary(frame(library(),'profiles.library',{clockId:'profiles:not-owner'})));
 assert.throws(()=>captureRuntime(frame({...runtime(),preferredProfiles:{'bad key':id}},'profiles.runtime')));
});
test('exact topics share one reduction and independent references dispose the last subscriber only',()=>{
 const h=harness(),first=[],second=[];const one=h.client.observeLibrary(s=>first.push(s)),two=h.client.observeLibrary(s=>second.push(s));assert.equal(h.opens.length,1);
 h.send(library());assert.equal(first.at(-1).value,second.at(-1).value);one();one();assert.equal(h.opens[0].signal.aborted,false);two();assert.equal(h.opens[0].signal.aborted,true);
 const stop=h.client.observeRuntime(()=>{});assert.equal(h.opens[1].topic,'profiles.runtime');stop();
 const again=h.client.observeLibrary(()=>{});assert.equal(h.opens.length,3);again();
});
test('loss and malformed replacements withdraw freshness until a new baseline; retired frames cannot revive values',()=>{
 const h=harness(),states=[];const off=h.client.observeLibrary(s=>states.push(s));h.send(library());
 h.send(undefined,{kind:'loss',continuity:'lost',generation:'2',revision:'2',sequence:'2',reason:'reset'});assert.equal(states.at(-1).status,'lost');
 h.send(library(),{kind:'change',continuity:'continuous',revision:'3',sequence:'3'});assert.equal(states.at(-1).status,'lost');
 h.send(library(),{generation:'2',revision:'3',sequence:'3'});assert.equal(states.at(-1).status,'ready');assert.equal(states.at(-1).generation,'2');
 h.send({...library(),versions:{}},{kind:'change',continuity:'continuous',generation:'2',revision:'4',sequence:'4'});assert.equal(states.at(-1).status,'error');
 h.send(library(),{kind:'change',continuity:'continuous',generation:'2',revision:'5',sequence:'5'});assert.equal(states.at(-1).status,'lost');
 h.send(library(),{generation:'2',revision:'6',sequence:'6'});assert.equal(states.at(-1).status,'ready');off();
});
test('a consumer exception cannot stall a second client or delivery acknowledgment',()=>{
 const h=harness(),states=[];const a=h.client.observeLibrary(()=>{throw Error('consumer');}),b=h.client.observeLibrary(s=>states.push(s));h.send(library());assert.equal(states.at(-1).status,'ready');a();b();
});

test('producer clock identity is pinned independently of broker session and generation',()=>{
 const h=harness(),states=[];const off=h.client.observeLibrary(s=>states.push(s));h.send(library());
 h.send(library(),{kind:'change',continuity:'continuous',revision:'2',sequence:'2',clockId:'profiles:22222222-2222-4222-8222-222222222222'});assert.equal(states.at(-1).status,'error');off();
});

const nextTurn=()=>new Promise(resolve=>setImmediate(resolve));
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};
async function until(condition){const deadline=Date.now()+5000;while(Date.now()<deadline){if(condition())return;await new Promise(resolve=>setTimeout(resolve,1));}assert.fail('Current profile-rule resolution did not settle.');}
function pagedRules(rows){
 const bytes=new TextEncoder().encode(JSON.stringify(rows)),manifest={id:`sha256:${createHash('sha256').update(bytes).digest('hex')}`,bytes:bytes.length,count:rows.length},calls=[];
 const get=async(id,offset,signal)=>{calls.push({id,offset,signal});let end=Math.min(offset+8192,bytes.length);while(end<bytes.length&&(bytes[end]&0xc0)===0x80)end--;return{id,offset,nextOffset:end,total:bytes.length,chunk:new TextDecoder().decode(bytes.subarray(offset,end))};};
 return{manifest,calls,get,payload:(patch={})=>({...runtime(),...patch,automatic:{...automatic,rules:{pages:manifest},...(patch.automatic??{})}})};
}
const rules=(count=1,size=10)=>Array.from({length:count},(_,n)=>({identity:{gameId:`game-${n}`,executable:`C:\\${'x'.repeat(size)}\\game.exe`},profileId:id,priority:n,enabled:true}));
test('shared runtime resolves >1MiB issued rules once for multiple consumers and applies the newest same-hash frame',async()=>{
 const f=pagedRules(rules(70,17000)),first=deferred();let requests=0;const h=harness(async(...args)=>{if(++requests===1)await first.promise;return f.get(...args);}),a=[],b=[];
 const offA=h.client.observeRuntime(state=>a.push(state)),offB=h.client.observeRuntime(state=>b.push(state));assert.equal(h.opens.length,1);h.send(f.payload());assert.equal(a.at(-1).status,'lost');
 h.send(f.payload({automatic:{message:'Latest native automatic decision'},preferredProfiles:{dcs:id}}),{kind:'change',continuity:'continuous',revision:'2',sequence:'2'});await nextTurn();assert.equal(requests,1);
 first.resolve();await until(()=>a.at(-1).status==='ready');const value=a.at(-1).value;assert.equal(value,b.at(-1).value);assert.equal(value.automatic.rules.length,70);assert.equal(value.automatic.message,'Latest native automatic decision');assert.equal(a.at(-1).revision,'2');assert.equal(Object.isFrozen(value.automatic.rules[0]),true);
 const count=requests;h.send(f.payload({automatic:{message:'Cached decision'}}),{kind:'change',continuity:'continuous',revision:'3',sequence:'3'});assert.equal(a.at(-1).status,'ready');assert.equal(a.at(-1).value.automatic.message,'Cached decision');assert.equal(requests,count);offA();assert.equal(h.opens[0].signal.aborted,false);offB();assert.equal(h.opens[0].signal.aborted,true);
});
test('new manifest replacement fences old page results and stops future pages for retired queries',async()=>{
 const old=pagedRules(rules(1,20000)),fresh=pagedRules(rules(2)),wait=deferred();let oldRequests=0;
 const h=harness(async(id,...rest)=>{if(id===old.manifest.id){oldRequests++;await wait.promise;return old.get(id,...rest);}return fresh.get(id,...rest);}),states=[],off=h.client.observeRuntime(state=>states.push(state));
 h.send(old.payload());await nextTurn();h.send(fresh.payload({automatic:{message:'Replacement'}}),{kind:'change',continuity:'continuous',revision:'2',sequence:'2'});await until(()=>states.at(-1).status==='ready');assert.equal(states.at(-1).value.automatic.rules.length,2);assert.equal(states.at(-1).revision,'2');wait.resolve();await nextTurn();assert.equal(oldRequests,1);assert.equal(old.calls[0].signal.aborted,true);assert.equal(states.at(-1).value.automatic.message,'Replacement');off();
});
test('source loss and last-unsubscribe cancel pending reconstruction, with no partial freshness or future page reads',async()=>{
 for(const retire of ['loss','unsubscribe']){
  const f=pagedRules(rules(1,20000)),wait=deferred();let requests=0;const h=harness(async(...args)=>{requests++;await wait.promise;return f.get(...args);}),states=[],off=h.client.observeRuntime(state=>states.push(state));h.send(f.payload());await nextTurn();
  if(retire==='loss')h.send(undefined,{kind:'loss',continuity:'lost',generation:'2',revision:'2',sequence:'2',reason:'reset'});else off();
  wait.resolve();await nextTurn();await nextTurn();assert.equal(requests,1);assert.notEqual(states.at(-1).status,'ready');assert.equal(f.calls[0].signal.aborted,true);
  if(retire==='loss'){h.send(runtime(),{generation:'2',revision:'3',sequence:'3'});assert.equal(states.at(-1).status,'ready');assert.equal(states.at(-1).generation,'2');off();}
 }
});
test('expired or corrupt rule pages remain explicit stale errors until a fresh current baseline; Stop remains independent',async()=>{
 const f=pagedRules(rules()),h=harness(async()=>{throw Error('Immutable profile-rule page expired.');}),states=[];
 const {ProfileRuntimeController}=await load('./profile-runtime-controller');let stops=0;const controller=new ProfileRuntimeController({status:async()=>runtime().profileSession,stop:async()=>{stops++;return runtime().profileSession;},recover:async()=>runtime().profileSession,apply:async()=>{throw Error('No observation may apply.');}});
 const off=h.client.observeRuntime(state=>{states.push(state);controller.observe(state);});h.send(f.payload());await until(()=>states.at(-1).status==='error');assert.match(states.at(-1).error,/expired/);assert.equal(controller.state.stale,true);
 assert.equal(await controller.stop(),true);assert.equal(stops,1);assert.equal(controller.state.stale,true);
 h.send(f.payload(),{kind:'change',continuity:'continuous',revision:'2',sequence:'2'});assert.equal(states.at(-1).status,'error');h.send(runtime(),{revision:'3',sequence:'3'});assert.equal(states.at(-1).status,'ready');assert.equal(controller.state.stale,false);off();controller.dispose();
 const bad=harness(async(...args)=>({...await f.get(...args),nextOffset:0})),badStates=[],done=bad.client.observeRuntime(s=>badStates.push(s));bad.send(f.payload());await until(()=>badStates.at(-1).status==='error');assert.match(badStates.at(-1).error,/Invalid/);done();
});

test('an expired manifest stays stale without repeated reads until a distinct current complete manifest arrives',async()=>{
 const old=pagedRules(rules(1)),fresh=pagedRules(rules(2));let expired=0;const h=harness(async(id,...rest)=>{if(id===old.manifest.id){expired++;throw Error('Immutable rules expired.');}return fresh.get(id,...rest);}),states=[],off=h.client.observeRuntime(s=>states.push(s));
 h.send(old.payload());await until(()=>states.at(-1).status==='error');h.send(old.payload({automatic:{message:'No repeated expired request'}}),{kind:'change',continuity:'continuous',revision:'2',sequence:'2'});await nextTurn();assert.equal(expired,1);assert.equal(states.at(-1).status,'error');
 h.send(fresh.payload({automatic:{message:'Current manifest'}}),{kind:'change',continuity:'continuous',revision:'3',sequence:'3'});await until(()=>states.at(-1).status==='ready');assert.equal(states.at(-1).value.automatic.rules.length,2);assert.equal(states.at(-1).revision,'3');off();
});
