import assert from 'node:assert/strict';
import test from 'node:test';
import {registerHooks} from 'node:module';
import {readFileSync} from 'node:fs';
registerHooks({load(url,context,next){if(url.endsWith('.rhai?raw'))return {format:'module',source:'export default '+JSON.stringify(readFileSync(new URL(url.replace(/\?raw$/,'')),'utf8')),shortCircuit:true};return next(url,context);}});
const {scriptExamples,scriptExample}=await import('./scriptExamples.ts');
const profile={roles:[{id:'wheel',controls:[{id:'shift',kind:{kind:'button'}},{id:'throttle',kind:{kind:'axis',range:'unipolar'}}]},{id:'stick',controls:[{id:'press',kind:{kind:'button'}}]}],targets:[{id:'main',layout:{buttons:2}}]};
test('examples bind declared compatible controls and leave the profile unchanged',()=>{
 const before=JSON.stringify(profile);
 for(const name of Object.keys(scriptExamples)) {
   const script=scriptExample(profile,name);
   assert.match(script.id,/^script-/);assert.deepEqual(script.parameters,{});assert.equal(script.outputs[0].output.target,'main');assert.equal(script.outputs[0].id,'result');
   assert.ok(script.source.includes('ctx.inputs["wheel"]'));
   assert.ok(script.inputs.every(i=>i.kind==='source'));
   assert.equal(script.inputs.length,['Shift layer','Two controls'].includes(name)?2:1);
   assert.ok(!script.source.includes('ctx.inputs.left.'));
 }
 assert.equal(JSON.stringify(profile),before);
});
test('missing control kinds or target buttons fail before creating a draft script',()=>{
 for(const name of Object.keys(scriptExamples))assert.throws(()=>scriptExample({roles:[],targets:profile.targets},name),/declared/);
 assert.throws(()=>scriptExample({...profile,targets:[]},'Timed pulse'),/target button/);
 assert.throws(()=>scriptExample({roles:[profile.roles[0]],targets:profile.targets},'Two controls'),/2 declared button inputs/);
});
