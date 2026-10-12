import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
const source=await readFile(new URL('./runtime-config.ts',import.meta.url),'utf8');
const exported={};
vm.runInNewContext(ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports:exported,TextEncoder});
const {configurationError,resizeMappings}=exported;
const config=()=>({name:'Rig é',expanderMask:1,buttons:[[24,1,0]],axes:[[1,49,10,4000,32768,1,1000]],hats:[[0,1,2,3]],encoders:[[4,5]],selectors:[[6,7]]});
test('complete independent controls and calibrated axes validate',()=>assert.equal(configurationError(config()),''));
test('contact collisions, unavailable expansion and maintained conflicts explain the input error',()=>{
  const duplicate=config();duplicate.buttons[0][0]=0;assert.match(configurationError(duplicate),/more than one/);
  const expansion=config();expansion.expanderMask=0;assert.match(configurationError(expansion),/expansion bank 1/);
  const maintained=config();maintained.buttons[0][2]=1;assert.match(configurationError(maintained),/maintained switches/i);
});
test('invalid calibration and duplicate axis sources are rejected before transmission',()=>{
  const invalid=config();invalid.axes[0][2]=invalid.axes[0][3];assert.match(configurationError(invalid),/minimum must be below/);
  const duplicate=config();duplicate.axes.push([1,48,0,4095,0,0,0]);assert.match(configurationError(duplicate),/unique ADC channel/);
});
test('adding an axis after reordering preserves existing mappings and chooses unused channel and usage',()=>{
  const existing=config();resizeMappings(existing,'axes',2);assert.deepEqual(existing.axes[0],[1,49,10,4000,32768,1,1000]);
  assert.deepEqual(JSON.parse(JSON.stringify(existing.axes[1])),[0,48,0,4095,0,0,0]);assert.equal(configurationError(existing),'');
});
