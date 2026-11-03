import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {ProfileRulePages,captureRulesManifest,MAX_RULE_BYTES} from './profile-read-models/rule-pages.ts';
const encode=value=>new TextEncoder().encode(JSON.stringify(value));
function fixture(rules,overrides={}) {
  const bytes=encode(rules),manifest={id:`sha256:${createHash('sha256').update(bytes).digest('hex')}`,bytes:bytes.length,count:rules.length,...overrides},calls=[];
  const get=async(id,offset)=>{calls.push({id,offset});let end=Math.min(offset+8192,bytes.length);while(end<bytes.length&&(bytes[end]&0xc0)===0x80)end--;
    return {id,offset,nextOffset:end,total:bytes.length,chunk:new TextDecoder().decode(bytes.subarray(offset,end))};};
  return{bytes,manifest,calls,get};
}
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};
const tick=()=>new Promise(resolve=>setImmediate(resolve));
test('more than 1MiB reconstructs exact UTF-8 rules and immutable per-hash cache deduplicates consumers',async()=>{
 const rows=Array.from({length:100},(_,n)=>({name:`rule-${n}`,executable:'C:\\'+('é'.repeat(8000))+'\\game.exe'})),f=fixture(rows),pages=new ProfileRulePages(f.get);
 assert.ok(f.bytes.length>1024*1024);const a=new AbortController(),b=new AbortController();const [one,two]=await Promise.all([pages.resolve(f.manifest,a.signal),pages.resolve(f.manifest,b.signal)]);
 assert.equal(one,two);assert.deepEqual(one,rows);assert.ok(Object.isFrozen(one[0]));const count=f.calls.length;assert.equal(await pages.resolve(f.manifest,new AbortController().signal),one);assert.equal(f.calls.length,count);assert.ok(f.calls.every(call=>call.id===f.manifest.id));
});
test('manifest boundaries reject malformed hashes, excessive bytes/counts and changed metadata for cached hashes',async()=>{
 const f=fixture([]),pages=new ProfileRulePages(f.get);assert.deepEqual(captureRulesManifest({pages:f.manifest}),f.manifest);
 for(const patch of [{id:'https://other'},{id:'sha256:ABC'},{bytes:MAX_RULE_BYTES+1},{bytes:1},{count:513},{count:-1},{count:0.5}])assert.throws(()=>captureRulesManifest({pages:{...f.manifest,...patch}}));
 assert.throws(()=>captureRulesManifest({pages:f.manifest,other:true}));await pages.resolve(f.manifest,new AbortController().signal);await assert.rejects(pages.resolve({...f.manifest,count:1},new AbortController().signal),/Invalid/);
});
test('page hash, count, offsets, ids, totals, chunk budgets and Unicode must match exactly before acceptance',async()=>{
 const cases=[page=>({...page,id:'sha256:'+'0'.repeat(64)}),page=>({...page,offset:1}),page=>({...page,nextOffset:0}),page=>({...page,nextOffset:page.nextOffset+1}),page=>({...page,total:page.total+1}),page=>({...page,chunk:'x'.repeat(8193)}),page=>({...page,chunk:'\ud800'}),page=>({...page,chunk:page.chunk.replace('valid','other')})];
 for(const change of cases){const f=fixture([{name:'valid'}]),pages=new ProfileRulePages(async(...args)=>change(await f.get(...args)));await assert.rejects(pages.resolve(f.manifest,new AbortController().signal));assert.equal(pages.cached(f.manifest),undefined);}
 const f=fixture([{name:'valid'}]),pages=new ProfileRulePages(f.get);await assert.rejects(pages.resolve({...f.manifest,count:2},new AbortController().signal),/Invalid/);
 const broken=fixture([], {id:'sha256:'+'0'.repeat(64)});await assert.rejects(new ProfileRulePages(broken.get).resolve(broken.manifest,new AbortController().signal),/Invalid/);
});
test('disposal stops future page reads and independent consumers share cancellation ownership',async()=>{
 const f=fixture([{text:'x'.repeat(20000)}]),wait=deferred();let requests=0;const pages=new ProfileRulePages(async(...args)=>{requests++;if(requests===1)await wait.promise;return f.get(...args);});
 const a=new AbortController(),b=new AbortController(),one=pages.resolve(f.manifest,a.signal),two=pages.resolve(f.manifest,b.signal);await tick();a.abort();await assert.rejects(one,/replaced/);wait.resolve();assert.ok(await two);assert.ok(requests>1);
 const waiting=deferred();requests=0;const cancelledPages=new ProfileRulePages(async(...args)=>{requests++;await waiting.promise;return f.get(...args);});const c=new AbortController(),pending=cancelledPages.resolve(f.manifest,c.signal);await tick();c.abort();await assert.rejects(pending,/replaced/);waiting.resolve();await tick();assert.equal(requests,1);assert.equal(cancelledPages.cached(f.manifest),undefined);
});
test('two cached blobs use LRU eviction, expired pages reject explicitly, and concurrent work is bounded',async()=>{
 const fixtures=[fixture([{id:1}]),fixture([{id:2}]),fixture([{id:3}])],lookup=new Map(fixtures.map(f=>[f.manifest.id,f])),wait=deferred();let active=0,peak=0;
 const pages=new ProfileRulePages(async(id,offset)=>{active++;peak=Math.max(peak,active);await wait.promise;try{return await lookup.get(id).get(id,offset);}finally{active--;}});
 const work=fixtures.map(f=>pages.resolve(f.manifest,new AbortController().signal));await tick();assert.equal(peak,2);wait.resolve();await Promise.all(work);assert.equal(pages.cached(fixtures[0].manifest),undefined);assert.ok(pages.cached(fixtures[1].manifest));assert.ok(pages.cached(fixtures[2].manifest));
 await assert.rejects(new ProfileRulePages(async()=>{throw Error('Immutable profile rule page expired.');}).resolve(fixtures[0].manifest,new AbortController().signal),/expired/);
});
