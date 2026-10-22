import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {validateRenderProfile} from '../src/render-profiles.ts';
const fixture=JSON.parse(await readFile(new URL('../../../apps/companion-esp32/test/render-profiles.json',import.meta.url)));
test('device renderer reports match shared compact/PSRAM contracts without mutating source evidence',()=>{
  for(const profile of fixture.valid){
    const before=JSON.stringify(profile);const result=validateRenderProfile(profile);assert.equal(result.ok,true);assert.equal(JSON.stringify(profile),before);
    result.value.features.length=0;assert.equal(JSON.stringify(profile),before);
    for(const mutation of fixture.mutations){
      const value=structuredClone(profile);let target=value;for(const key of mutation.path.slice(0,-1))target=target[key];target[mutation.path.at(-1)]=mutation.value;
      assert.equal(validateRenderProfile(value).ok,mutation.valid,mutation.path.join('.'));
    }
  }
  for(const invalid of [undefined,null,[],{extra:'x'.repeat(4096)}])assert.equal(validateRenderProfile(invalid).ok,false);
  const cycle={};cycle.cycle=cycle;assert.equal(validateRenderProfile(cycle).ok,false);
});
