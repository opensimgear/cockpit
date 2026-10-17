import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {setterProfile} from './profile-graph-fixtures.mjs';
import {fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
const root=process.env.OSG_ACCEPTANCE_ROOT ?? fileURLToPath(new URL('../../',import.meta.url));
const blank=JSON.parse(await readFile(resolve(root,'tests/fixtures/profiles/blank.json'),'utf8'));
const contract={delivery:'action',parameters:[{id:'color',name:'RGB',schema:{type:'rgb'}}],semantics:'setter',endpoint:'rgb-led',readable:true,display:null,ownership:{scope:'endpoint',maxLeaseMs:5000,restorePolicy:'if-unchanged',fallbackPolicy:'capture-at-claim'}};
const observed={descriptor:{id:'action:led.rgb.set_color',name:'RGB',direction:'input',contract},contractHash:'sha256:'+'a'.repeat(64)};
const resource={id:'11111111-1111-4111-8111-111111111111'};
test('physical builder preserves exact typed setter and local pins without portable machine identity',async()=>{
 const result=setterProfile(blank,resource,observed,'445566');
 assert.equal(result.document.schemaVersion,2);assert.equal(result.document.resources[0].required,true);
 assert.deepEqual(result.document.resources[0].ports[0].contract,contract);
 assert.deepEqual(result.document.processingNodes[1].operation,{kind:'set-on-change',arguments:['color']});
 assert.equal(result.document.processingNodes[1].outputs[0].contract.semantics,'setter');
 assert.deepEqual(result.assignments,[{resourceId:'panel',sourceId:resource.id,ports:[{profilePortId:'set',observedPortId:observed.descriptor.id,expectedContractHash:observed.contractHash}]}]);
 assert.ok(!JSON.stringify(result.document).includes(resource.id));assert.ok(!JSON.stringify(result.document).includes(observed.contractHash));
 if(process.env.OSG_PROFILE_FIXTURE_OUTPUT)await writeFile(process.env.OSG_PROFILE_FIXTURE_OUTPUT,JSON.stringify(result,null,2));
});
test('legacy unguarded and host-fallback-only descriptors cannot create an owned output graph',()=>{
 for(const transform of [c=>delete c.ownership,c=>delete c.ownership.restorePolicy,c=>delete c.ownership.fallbackPolicy,c=>c.readable=false,c=>c.semantics='one-shot']){
  const altered=structuredClone(observed);transform(altered.descriptor.contract);assert.throws(()=>setterProfile(blank,resource,altered,'445566'));
 }
});
test('native parameter named requested does not collide with invocation output',()=>{
 const altered=structuredClone(observed);altered.descriptor.contract.parameters[0].id='requested';
 const result=setterProfile(blank,resource,altered,'445566');const node=result.document.processingNodes[1];
 assert.notEqual(node.inputs[0].id,node.outputs[0].id);assert.equal(result.document.bindings[1].from.portId,node.outputs[0].id);
});
