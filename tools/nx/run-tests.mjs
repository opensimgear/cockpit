import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import plugin from './plugin.cjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const manifest = plugin._private.readManifest(root);
const groupName = process.argv[2];
const group = manifest.groups.find(item => item.name === groupName);

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

if (!group) fail(`Unknown Nx test group: ${groupName ?? '(missing)'}`);
const tests = plugin._private.testFilesForGroup(root, group, manifest.groups);
if (!tests.length) fail(`No portable tests found for ${group.name}; refusing to pass an empty target.`);

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit', shell: plugin._private.shouldUseShell(command) });
  if (result.error) fail(`Could not start ${command}: ${result.error.message}`);
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function scriptTestFiles(script) {
  const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const command = packageJson.scripts?.[script];
  if (!command) fail(`Coverage script '${script}' does not exist in package.json.`);
  const found = new Set();
  for (const token of command.match(/(?:src|apps|tests)\/[^\s"']+\.test\.mjs/g) ?? []) {
    if (!token.includes('*') && fs.existsSync(path.join(root, token))) found.add(token);
    else if (token.includes('*')) {
      for (const test of tests) if (plugin._private.globRegex(token).test(test)) found.add(test);
    }
  }
  return found;
}

const coveredTests = new Set();
for (const script of group.coverageScripts ?? []) {
  for (const file of scriptTestFiles(script)) coveredTests.add(file);
  run('pnpm', ['run', script]);
}

for (const task of group.coverageTasks ?? []) {
  for (const file of task.tests ?? []) coveredTests.add(file);
  run(process.execPath, task.arguments);
}

const remainingTests = tests.filter(file => !coveredTests.has(file));
if (remainingTests.length) run(process.execPath, ['--test-concurrency=1', '--test', ...remainingTests]);
