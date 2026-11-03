import assert from'node:assert/strict';import test from'node:test';import{createHash,webcrypto}from'node:crypto';import{readFile}from'node:fs/promises';import{fileURLToPath}from'node:url';import vm from'node:vm';import ts from'typescript';import{discoveredGames}from'../game-test-fixtures.mjs';
const modules=new Map();async function load(specifier,parent=new URL('../',import.meta.url)){
 const url=new URL(specifier.endsWith('.ts')?specifier:`${specifier}.ts`,parent),key=url.href;if(modules.has(key))return modules.get(key);
 if(url.pathname.endsWith('/streams/transport.ts')){const value={createObservationTransport(){throw Error('Unexpected default transport.');}};modules.set(key,value);return value;}
 const code=ts.transpileModule(await readFile(url,'utf8'),{fileName:fileURLToPath(url),compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,inlineSourceMap:true,inlineSources:true}}).outputText;
 const dependencies=new Map();for(const match of code.matchAll(/require\(["']([^"']+)["']\)/g))dependencies.set(match[1],await load(match[1],url));
 const exports={};vm.runInNewContext(code,{exports,structuredClone,TextEncoder,TextDecoder,AbortController,crypto:webcrypto,require:name=>dependencies.get(name)},{filename:fileURLToPath(url)});modules.set(key,exports);return exports;
}
const games=await load('./games'),{updateGameCatalogue}=games;updateGameCatalogue(discoveredGames);
const{ProfileNodeSamplesObservers}=await load('./shared',new URL('./',import.meta.url));
const{profileNodeSamplesTopic,captureProfileNodeSamples}=await load('./models',new URL('./',import.meta.url));
const{SharedObservations}=await load('./streams/shared',new URL('../',import.meta.url));
const id='11111111-1111-4111-8111-111111111111',session='22222222-2222-4222-8222-222222222222',hash=`sha256:${'a'.repeat(64)}`;
const owner=()=>({session,generation:3,profileId:id,profileRevision:7,gameId:'dcs'}),payload=()=>({owner:owner(),nodeId:'aircraft',appliedSnapshot:{profileId:id,revision:7,semanticHash:hash},programHash:hash,ports:[{endpoint:{nodeId:'aircraft',portId:'enabled'},direction:'input',contract:{delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false},sample:{status:'known',value:{kind:'scalar',value:false}}}]});
const digest=async bytes=>webcrypto.subtle.digest('SHA-256',bytes),frame=(topic,data,changes={})=>({kind:'snapshot',topic,source:'profile-samples',session,generation:'1',revision:'1',sequence:'1',capturedUs:'1',clockId:`profile-samples:${session}`,continuity:'baseline',payload:data,...changes});
function harness(){const opens=[],observations=new SharedObservations({open(topic,observer,signal){opens.push({topic,observer,signal,delivery:0});}});return{opens,client:new ProfileNodeSamplesObservers(observations,digest),send(data,options={},entry=opens[0]){entry.observer.frame({subscriptionId:entry.topic,delivery:String(++entry.delivery),frame:frame(entry.topic,data,options)},async()=>{});}};}
const tick=()=>new Promise(resolve=>setTimeout(resolve,0));
async function until(condition){const deadline=Date.now()+2000;while(Date.now()<deadline){if(condition())return;await tick();}assert.fail('Sample observation did not open.');}
test('topic uses exact ordered owner tuple and matches the native lowercase SHA-256 vector',async()=>{const vectorOwner={session:'b3738156-6794-42dc-978b-2376dce0c6c6',generation:7,profileId:id,profileRevision:3,gameId:'dcs'};const vector=JSON.stringify(['b3738156-6794-42dc-978b-2376dce0c6c6',7,id,3,'dcs','virtual-1']);assert.equal(createHash('sha256').update(vector).digest('hex'),'f9d15e05c22364b7a5f70a60116ab984faaea8bce05609245c29a7326045822b');assert.equal(await profileNodeSamplesTopic(vectorOwner,'virtual-1',digest),'profiles.samples.f9d15e05c22364b7a5f70a60116ab984faaea8bce05609245c29a7326045822b');assert.equal(await profileNodeSamplesTopic(owner(),'aircraft',digest),`profiles.samples.${createHash('sha256').update(JSON.stringify([session,3,id,7,'dcs','aircraft'])).digest('hex')}`);await assert.rejects(profileNodeSamplesTopic(owner(),'bad/node',digest));});
test('value-only samples validate exact owner/node, reject duplicates and share immutable values',async()=>{
 const h=harness(),a=[],b=[],one=h.client.observe(owner(),'aircraft',state=>a.push(state)),two=h.client.observe(owner(),'aircraft',state=>b.push(state));await until(()=>h.opens.length===1);assert.equal(h.opens[0].topic,await profileNodeSamplesTopic(owner(),'aircraft',digest));
 h.send(payload());assert.equal(a.at(-1).status,'ready');assert.equal(a.at(-1).value,b.at(-1).value);assert.equal(a.at(-1).value.ports[0].sample.value.value,false);assert.equal(Object.isFrozen(a.at(-1).value.ports[0].sample),true);assert.equal('observedUs'in a.at(-1).value.ports[0],false);
 const wrong=payload();wrong.ports.push(structuredClone(wrong.ports[0]));assert.throws(()=>captureProfileNodeSamples(frame(h.opens[0].topic,wrong),h.opens[0].topic,owner(),'aircraft'),/invalid port|duplicate/);
 const other=payload();other.owner.generation++;assert.throws(()=>captureProfileNodeSamples(frame(h.opens[0].topic,other),h.opens[0].topic,owner(),'aircraft'),/applied owner/);
 const badHash=payload();badHash.programHash='sha256:bad';assert.throws(()=>captureProfileNodeSamples(frame(h.opens[0].topic,badHash),h.opens[0].topic,owner(),'aircraft'),/applied owner/);
 const badNode=payload();badNode.nodeId='other';assert.throws(()=>captureProfileNodeSamples(frame(h.opens[0].topic,badNode),h.opens[0].topic,owner(),'aircraft'),/this node/);
 one();assert.equal(h.opens[0].signal.aborted,false);two();assert.equal(h.opens[0].signal.aborted,true);
});
test('loss withdraws samples until fresh baseline and delayed changes cannot revive data',async()=>{
 const h=harness(),states=[],off=h.client.observe(owner(),'aircraft',state=>states.push(state));await until(()=>h.opens.length===1);h.send(payload());assert.equal(states.at(-1).status,'ready');
 h.send(undefined,{kind:'loss',continuity:'lost',generation:'2',revision:'2',sequence:'2',reason:'reset'});assert.equal(states.at(-1).status,'lost');
 h.send(payload(),{kind:'change',continuity:'continuous',generation:'2',revision:'3',sequence:'3'});assert.equal(states.at(-1).status,'lost');
 h.send(payload(),{generation:'2',revision:'4',sequence:'4'});assert.equal(states.at(-1).status,'ready');off();
});
test('owner/node completion after cleanup cannot open a retired subscription',async()=>{
 let release;const delayed=()=>new Promise(resolve=>{release=resolve;}),opens=[],observations=new SharedObservations({open(topic,observer,signal){opens.push({topic,observer,signal});}}),client=new ProfileNodeSamplesObservers(observations,delayed),off=client.observe(owner(),'aircraft',()=>{});off();release(await digest(new TextEncoder().encode(JSON.stringify([session,3,id,7,'dcs','aircraft']))));await tick();assert.equal(opens.length,0);
});
test('reentrant listeners share one open and synchronous cleanup aborts the just-created transport',async()=>{
 const h=harness();let nested,outer;outer=h.client.observe(owner(),'aircraft',state=>{if(state.status==='connecting'&&!nested)nested=h.client.observe(owner(),'aircraft',()=>{});});await until(()=>h.opens.length===1);assert.equal(h.opens.length,1);outer();nested();assert.equal(h.opens[0].signal.aborted,true);
 let stopped=false,subscribed=false;const synchronous=new ProfileNodeSamplesObservers({subscribe(topic,listener){subscribed=true;listener({status:'error',error:'synchronous'});return()=>{stopped=true;};}},digest);let dispose;dispose=synchronous.observe(owner(),'aircraft',state=>{if(state.status==='error')dispose();});await until(()=>subscribed);assert.equal(stopped,true);
});
