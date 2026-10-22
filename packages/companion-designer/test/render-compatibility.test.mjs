import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {renderCompatibility} from '../src/render-compatibility.ts';
const fixture=JSON.parse(await readFile(new URL('../../../apps/companion-esp32/test/widget-compatibility.json',import.meta.url)));
const profiles=JSON.parse(await readFile(new URL('../../../apps/companion-esp32/test/render-profiles.json',import.meta.url))).valid;

test('target compatibility preserves source and uses actual portable target limits',()=>{
  for(const item of fixture.cases){
    const document=JSON.parse(item.canonicalDocument),before=JSON.stringify(document),profile=structuredClone(profiles[item.profile]);
    assert.deepEqual(renderCompatibility(document,profile),item.issues,item.name);
    assert.equal(JSON.stringify(document),before,item.name);assert.deepEqual(profile,profiles[item.profile]);
  }
  const byName=name=>fixture.cases.find(item=>item.name===name);
  assert.equal(byName('rich catalog').issues.length,0);
  for(const name of ['complete rich catalog','compact text instrument','compact original icon'])assert.equal(byName(name).issues.length,0,name);
  assert.ok(byName('compact catalog rejects retained budgets').issues.some(issue=>issue.field==='components'));
  assert.equal(byName('compact reusable switch').issues.length,0);
  assert.ok(byName('alternate-state unsupported glyph').issues.some(issue=>issue.layer&&issue.definition&&issue.widget&&issue.required==='U+1F600'));
  assert.equal(byName('same glyph supported by mono').issues.length,0);
  assert.equal(renderCompatibility({},profiles[1])[0].field,'source');
  assert.equal(renderCompatibility(JSON.parse(byName('rich catalog').canonicalDocument),{})[0].field,'renderProfile');
});
