import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import plugin from './plugin.cjs';

const root = path.resolve(import.meta.dirname, '../..');
const manifest = plugin._private.readManifest(root);
const groups = manifest.groups;

function walk(dir, found = []) {
  if (!fs.existsSync(dir)) return found;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(file, found);
    else if (entry.name.endsWith('.test.mjs')) found.push(path.relative(root, file).split(path.sep).join('/'));
  }
  return found;
}

test('every src/lib portable test has one and only one feature owner', () => {
  const files = walk(path.join(root, 'src/lib')).filter(file => file.endsWith('.test.mjs'));
  assert.ok(files.length > 0);
  for (const file of files) {
    const owners = groups.filter(group => !group.catchAll && group.testPatterns.some(pattern => plugin._private.globRegex(pattern).test(file)) && !(group.excludePatterns ?? []).some(pattern => plugin._private.globRegex(pattern).test(file)));
    assert.ok(owners.length <= 1, `${file} has multiple owners: ${owners.map(group => group.name).join(', ')}`);
    assert.ok(plugin._private.featureForTest(file, groups), `${file} has no owner`);
  }
});

test('new test names follow stable ownership rules and shell is the fallback', () => {
  assert.equal(plugin._private.featureForTest('src/lib/profile-new-capability.test.mjs', groups), 'profiles-fe');
  assert.equal(plugin._private.featureForTest('src/lib/game-new-capability.test.mjs', groups), 'games-fe');
  assert.equal(plugin._private.featureForTest('src/lib/unclassified-new-capability.test.mjs', groups), 'shell-fe');
});

test('workspace-root quoted reads resolve alongside relative URLs and imports', () => {
  assert.deepEqual(plugin._private.resolveRelative('src/lib/example.test.mjs', 'src/lib/ProfilesPanel.svelte', root), ['src/lib/ProfilesPanel.svelte']);
  assert.deepEqual(plugin._private.resolveRelative('tests/example.test.mjs', '../src/lib/ProfilesPanel.svelte', root), ['src/lib/ProfilesPanel.svelte']);
  assert.equal(plugin._private.testFilesForGroup(root, groups.find(group => group.name === 'shell-fe'), groups).includes('scripts/native-gateway-bridge.test.mjs'), true);
});

test('browser-companion app tests remain owned by their app project', () => {
  const group = groups.find(item => item.name === 'companions-fe');
  const tests = plugin._private.testFilesForGroup(root, group, groups);
  assert.ok(tests.every(file => file.startsWith('src/lib/') || group.additionalTests?.includes(file)));
  assert.ok(!plugin._private.targetInputs(root, manifest, group, groups).some(input => input.startsWith('{workspaceRoot}/apps/browser-companion/')));
});

test('Windows runs Node directly even when its executable path contains spaces', () => {
  assert.equal(plugin._private.shouldUseShell('C:\\Program Files\\nodejs\\node.exe', 'win32'), false);
  assert.equal(plugin._private.shouldUseShell('pnpm', 'win32'), true);
  const child = spawnSync(process.execPath, ['-e', 'process.exit(0)'], { cwd: root, shell: false });
  assert.equal(child.error, undefined, child.error?.message);
  assert.equal(child.status, 0, child.stderr?.toString());
});

test('the profile UI reader closure includes real Svelte inputs', () => {
  const profile = groups.find(group => group.name === 'profiles-fe');
  const tests = plugin._private.testFilesForGroup(root, profile, groups);
  const closure = plugin._private.sourceClosure(root, tests);
  assert.ok(closure.includes('src/lib/ProfilesPanel.svelte'));
  const inputs = plugin._private.targetInputs(root, manifest, profile, groups);
  assert.ok(inputs.includes('{workspaceRoot}/src/lib/ProfilesPanel.svelte'));
  assert.ok(inputs.includes('{workspaceRoot}/src/App.svelte'));
  // profile-overview-ui.test.mjs has both a recursive component loader and a
  // separate App harness with stubs, so the safe union includes App children.
  assert.ok(inputs.includes('{workspaceRoot}/src/lib/SystemDevices.svelte'));
  const devices = groups.find(group => group.name === 'devices-fe');
  const deviceInputs = plugin._private.targetInputs(root, manifest, devices, groups);
  assert.ok(deviceInputs.includes('{workspaceRoot}/src/lib/System*.svelte'));
  const mockedGatewayUiTest = plugin._private.sourceClosure(root, ['src/lib/game-profile-context-ui.test.mjs']);
  assert.ok(!mockedGatewayUiTest.includes('src/lib/gateway.ts'), 'Svelte imports stubbed by the test are not actual file reads');
  assert.ok(!mockedGatewayUiTest.some(file => file.startsWith('packages/')), 'type-only module imports do not add unexecuted package reads');
});

test('Svelte closure follows live helpers, omits explicit stubs, and keeps independent readers', t => {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'osg-nx-closure-'));
  t.after(() => fs.rmSync(fixture, { recursive: true, force: true }));
  function write(name, source) {
    const file = path.join(fixture, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, source);
  }
  write('src/Panel.svelte', `<script>import { helper } from './helper'; import { gatewayRequest } from './gateway'; import type { Shape } from './types';</script><p>{helper()}</p>`);
  write('src/helper.ts', 'export const helper = () => 1;');
  write('src/gateway.ts', 'export const gatewayRequest = () => 1;');
  write('src/types.ts', 'export type Shape = number;');
  write('src/panel.test.mjs', `const source = readFile(new URL('./Panel.svelte', import.meta.url)); compile(source); function require(name) { if (name === './gateway') return { gatewayRequest() {} }; throw Error(name); }`);
  const panel = plugin._private.sourceClosure(fixture, ['src/panel.test.mjs']);
  assert.ok(panel.includes('src/Panel.svelte'));
  assert.ok(panel.includes('src/helper.ts'), 'an executable Svelte helper must invalidate the test');
  assert.ok(!panel.includes('src/gateway.ts'), 'an object-literal component stub does not execute the gateway');
  assert.ok(!panel.includes('src/types.ts'), 'type-only imports do not execute');
  write('src/gateway.test.mjs', `readFile(new URL('./gateway.ts', import.meta.url));`);
  const both = plugin._private.sourceClosure(fixture, ['src/panel.test.mjs', 'src/gateway.test.mjs']);
  assert.ok(both.includes('src/gateway.ts'), 'an independent real reader overrides another test stub');
});

test('App child imports follow only the current test stubs', t => {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'osg-nx-app-'));
  t.after(() => fs.rmSync(fixture, { recursive: true, force: true }));
  function write(name, source) {
    const file = path.join(fixture, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, source);
  }
  write('src/App.svelte', `<script>import Child from './lib/Child.svelte'; import { state } from './lib/state';</script><Child />`);
  write('src/lib/Child.svelte', `<script>import { childState } from './child-state';</script><p>{childState}</p>`);
  write('src/lib/state.ts', 'export const state = 1;');
  write('src/lib/child-state.ts', 'export const childState = 1;');
  write('src/stub.test.mjs', `const app = readFile(new URL('./App.svelte', import.meta.url)); compile(app); function require(name) { if (name.endsWith('.svelte')) return { default() {} }; }`);
  write('src/real.test.mjs', `const app = readFile(new URL('./App.svelte', import.meta.url)); compile(app);`);
  const stubbed = plugin._private.sourceClosure(fixture, ['src/stub.test.mjs']);
  assert.ok(stubbed.includes('src/App.svelte'));
  assert.ok(stubbed.includes('src/lib/state.ts'), 'App executable helpers remain inputs even when panels are stubbed');
  assert.ok(!stubbed.includes('src/lib/Child.svelte'));
  const real = plugin._private.sourceClosure(fixture, ['src/real.test.mjs']);
  assert.ok(real.includes('src/lib/Child.svelte'));
  assert.ok(real.includes('src/lib/child-state.ts'));
  write('src/mixed.test.mjs', `async function component(name) { for (const match of code.matchAll(/from/g)) await component(match[1]); } const app = readFile(new URL('./App.svelte', import.meta.url)); compile(app); function require(name) { if (name.endsWith('.svelte')) return { default() {} }; }`);
  const mixed = plugin._private.sourceClosure(fixture, ['src/mixed.test.mjs']);
  assert.ok(mixed.includes('src/lib/Child.svelte'), 'a generic stub elsewhere in the test cannot hide recursive real children');
});

test('closure includes cwd reads and unresolved import candidates for added or deleted files', t => {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'osg-nx-files-'));
  t.after(() => fs.rmSync(fixture, { recursive: true, force: true }));
  fs.mkdirSync(path.join(fixture, 'src'), { recursive: true });
  fs.mkdirSync(path.join(fixture, 'tests/fixtures'), { recursive: true });
  fs.writeFileSync(path.join(fixture, 'tests/fixtures/data.json'), '{}');
  fs.writeFileSync(path.join(fixture, 'src/reader.test.mjs'), `readFile('tests/fixtures/data.json'); import './new-module';`);
  const before = plugin._private.sourceClosure(fixture, ['src/reader.test.mjs']);
  assert.ok(before.includes('tests/fixtures/data.json'));
  assert.ok(before.includes('src/new-module.ts'), 'a newly added module must match a preexisting input');
  fs.writeFileSync(path.join(fixture, 'src/new-module.ts'), 'export const value = 1;');
  const added = plugin._private.sourceClosure(fixture, ['src/reader.test.mjs']);
  assert.ok(added.includes('src/new-module.ts'));
  fs.rmSync(path.join(fixture, 'src/new-module.ts'));
  const deleted = plugin._private.sourceClosure(fixture, ['src/reader.test.mjs']);
  assert.ok(deleted.includes('src/new-module.ts'));
});

test('Nx nodes keep coverage inside one portable test target and never invoke native UI suites', () => {
  const nodes = plugin._private.createProjectNodes(root);
  assert.deepEqual(Object.values(nodes).map(project => project.name).sort(), [...groups.map(group => group.name), 'nx-workspace'].sort());
  for (const group of groups) {
    const targets = Object.values(nodes).find(project => project.name === group.name).targets;
    assert.deepEqual(Object.keys(targets), ['test']);
    assert.match(targets.test.command, /^node tools\/nx\/run-tests\.mjs /);
    assert.ok(!targets.test.command.includes('native:ui'));
    assert.ok(targets.test.inputs.length > 0);
  }
  const workspace = Object.values(nodes).find(project => project.name === 'nx-workspace');
  assert.match(workspace.targets.test.command, /--test-coverage-include=tools\/nx\/\*\*\/\*\.cjs/);
  assert.match(workspace.targets.test.command, /--test-coverage-lines=71 --test-coverage-branches=71 --test-coverage-functions=71/);
  assert.deepEqual(workspace.targets.test.inputs, ['sharedGlobals']);
});

test('Nx inference is attached to one manifest and returns deterministic project metadata', async () => {
  const [pattern, infer] = plugin.createNodes;
  assert.equal(pattern, 'tools/nx/workspace.json');
  assert.deepEqual(await infer(['unrelated.json'], {}, { workspaceRoot: root }), []);
  const result = await infer(['tools/nx/workspace.json'], {}, { workspaceRoot: root });
  assert.equal(result.length, 1);
  assert.equal(result[0][0], 'tools/nx/workspace.json');
  assert.deepEqual(Object.keys(result[0][1].projects).sort(), Object.keys(plugin._private.createProjectNodes(root)).sort());
});

test('existing root coverage scripts do not execute the same portable test twice', () => {
  const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  for (const group of groups) {
    const tests = new Set(plugin._private.testFilesForGroup(root, group, groups));
    const seen = new Set();
    for (const scriptName of group.coverageScripts ?? []) {
      const command = packageJson.scripts[scriptName];
      assert.ok(command, `missing package script ${scriptName}`);
      for (const token of command.match(/(?:src|apps|tests)\/[^\s"']+\.test\.mjs/g) ?? []) {
        if (token.includes('*')) {
          for (const file of tests) if (plugin._private.globRegex(token).test(file)) {
            assert.ok(!seen.has(file), `${file} is covered by more than one script in ${group.name}`);
            seen.add(file);
          }
        } else if (tests.has(token)) {
          assert.ok(!seen.has(token), `${token} is covered by more than one script in ${group.name}`);
          seen.add(token);
        }
      }
    }
    for (const task of group.coverageTasks ?? []) {
      for (const file of task.tests ?? []) {
        assert.equal(plugin._private.featureForTest(file, groups), group.name, `${file} is covered from a different feature task`);
        assert.ok(!seen.has(file), `${file} is covered by both a package script and an inline coverage task in ${group.name}`);
        seen.add(file);
      }
    }
  }
});
