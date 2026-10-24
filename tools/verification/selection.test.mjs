import test from 'node:test';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {join} from 'node:path';

const root=fileURLToPath(new URL('../../',import.meta.url));
const cli=join(root,'node_modules/nx',JSON.parse(readFileSync(join(root,'node_modules/nx/package.json'))).bin.nx);
const env={...process.env,NX_DAEMON:'false',NX_ISOLATE_PLUGINS:'false',NX_NO_CLOUD:'true'};
function affected(file,target='test') {
  const text=execFileSync(process.execPath,[cli,'show','projects','--affected',`--withTarget=${target}`,`--files=${file}`,'--json'],{cwd:root,env,encoding:'utf8',timeout:60000});
  return JSON.parse(text);
}

test('Profiles UI selects its tests without running browser or package suites',()=>{
  const selected=affected('src/lib/ProfilesPanel.svelte');
  assert.ok(selected.includes('profiles-fe'));
  for(const unrelated of ['browser-companion','mobile-companion','widget-runtime','widget-compiler','companion-contracts','companion-designer','companion-ui'])assert.ok(!selected.includes(unrelated),unrelated);
});
test('browser feedback selects only its frontend tests',()=>{
  assert.deepEqual(affected('apps/browser-companion/src/feedback.ts'),['browser-companion']);
});
test('a new or deleted profile test is selected by its ownership pattern',()=>{
  assert.ok(affected('src/lib/profile-nx-selection-new.test.mjs').includes('profiles-fe'));
});
test('fixture byte changes select the generator consistency gate',()=>{
  assert.ok(affected('scripts/fixtures/compiled-widgets.json','check').includes('shared-fixtures'));
  assert.ok(affected('tests/fixtures/legacy-companion/shared-systems.bundle.json','check').includes('shared-fixtures'));
});
test('native persistence changes reach the dependent platform gates',()=>{
  const selected=affected('crates/persistence/src/connection.rs','native');
  for(const expected of ['native-persistence','native-host','native-controllers','native-windows-package'])assert.ok(selected.includes(expected),expected);
});
test('ordinary documentation changes select no executable test suites',()=>{
  assert.deepEqual(affected('docs/profiles-guide.md'),[]);
});
