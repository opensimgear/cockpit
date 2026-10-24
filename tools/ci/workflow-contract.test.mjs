import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { parse } from 'yaml';

const workflow = (name) => readFileSync(new URL(`../../.github/workflows/${name}`, import.meta.url), 'utf8');
const workflowData = (name) => parse(workflow(name));

test('changed workflow files parse as YAML', () => {
  for (const name of ['ci.yml', 'ci-native.yml', 'daily.yml', 'companion-mobile.yml', 'companion-esp32.yml', 'cockpit-windows-release.yml', 'component-release.yml']) {
    assert.ok(workflowData(name), `${name} did not parse`);
  }
});

test('fast CI runs affected Nx check, test and build targets', () => {
  const ci = workflow('ci.yml');
  assert.match(ci, /nx affected --target=check,test,build/);
  assert.match(ci, /actions: read/);
  assert.match(ci, /baseline\.mjs/);
});

test('native reusable workflow retains every previous native CI job', () => {
  const native = workflowData('ci-native.yml');
  assert.deepEqual(Object.keys(native.jobs), [
    'persistence-contracts', 'profiles-contracts', 'native', 'mapping-prototypes', 'controllers', 'mapping',
  ]);
  for (const [job, input] of [
    ['persistence-contracts', 'run_persistence'],
    ['profiles-contracts', 'run_profiles'],
    ['native', 'run_host'],
    ['mapping-prototypes', 'run_logic'],
    ['controllers', 'run_controllers'],
    ['mapping', 'run_mapping'],
  ]) assert.equal(native.jobs[job].if, `\${{ inputs.${input} }}`);
});

test('daily workflow selects changed native projects and gates every selected workflow', () => {
  const daily = workflow('daily.yml');
  const parsed = workflowData('daily.yml');
  for (const marker of ['native-host', 'native-controllers', 'native-profiles', 'native-persistence', 'native-mapping', 'native-logic', 'native-mobile', 'native-esp32', 'native-windows-package']) {
    assert.ok(daily.includes(marker) || daily.includes(`native_${marker.replace('native-', '').replaceAll('-', '_')}`), `missing daily selector for ${marker}`);
  }
  assert.match(daily, /daily-gate\.mjs/);
  assert.match(daily, /needs: \[baseline, select-native, native-tests, mobile, esp32, windows-package\]/);
  assert.deepEqual(parsed.jobs['daily-gate'].needs, ['baseline', 'select-native', 'native-tests', 'mobile', 'esp32', 'windows-package']);
  assert.equal(parsed.jobs['daily-gate'].if, 'always()');
});

test('expensive package workflows do not trigger directly on every push', () => {
  for (const name of ['companion-mobile.yml', 'companion-esp32.yml', 'cockpit-windows-release.yml']) {
    assert.equal(Object.hasOwn(workflowData(name).on, 'push'), false, `${name} still has a push trigger`);
  }
});

test('ESP32 qualification remains intentionally deferred', () => {
  const esp = workflow('companion-esp32.yml');
  const parsed = workflowData('companion-esp32.yml');
  assert.equal((esp.match(/if: \$\{\{ false \}\}/g) ?? []).length, 5);
  assert.match(esp, /workflow_call/);
  assert.equal(Object.values(parsed.jobs).filter((job) => job.if === '${{ false }}').length, 5);
});

test('tagged component release trigger and publishing gates remain intact', () => {
  const release = workflow('component-release.yml');
  const parsed = workflowData('component-release.yml');
  assert.deepEqual(parsed.on.push.tags, ['cockpit-v*', 'esp32-companion-v*', 'mobile-companion-v*']);
  for (const tag of ['cockpit-v*', 'esp32-companion-v*', 'mobile-companion-v*']) assert.ok(release.includes(tag));
  assert.match(release, /Publish complete component release/);
  assert.match(release, /Announce published component release/);
});
