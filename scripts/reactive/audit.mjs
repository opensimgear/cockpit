// Inventory timing sites, including legitimate leases/deadlines; never a timer ban.
import {execFileSync} from 'node:child_process';
import {readFileSync, writeFileSync} from 'node:fs';
import {dirname, resolve, relative} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const check = process.argv.includes('--check');
const committed = process.argv.includes('--committed');
const revisionAt = cwd => committed
  ? execFileSync('git', ['rev-parse', 'HEAD'], {cwd, encoding:'utf8'}).trim()
  : undefined;
const filesFromGit = (cwd, revision) => execFileSync('git', revision
  ? ['ls-tree', '-r', '--name-only', '-z', revision]
  : ['ls-files', '--cached', '--others', '--exclude-standard', '-z'],
{cwd, encoding:'utf8'}).split('\0');
const sourceAt = (cwd, file, revision) => revision
  ? execFileSync('git', ['show', `${revision}:${file}`], {cwd, encoding:'utf8'})
  : readFileSync(resolve(cwd, file), 'utf8');

if (process.argv.includes('--check-ui-timers')) {
  const { checkUiTimers } = await import('./ui-timers.mjs');
  const revision = revisionAt(root);
  const files = [...new Set(filesFromGit(root, revision))].filter(file =>
    /^(?:src|apps|packages)\//.test(file) && /\.(?:[cm]?[jt]s|[jt]sx|svelte)$/.test(file)
    && !/(?:^|\/)(?:tests?|fixtures|dist|target|vendor|simulation)(?:\/|$)|(?:^|\/)test-fixtures?\.|[._-](?:tests?|fixtures)\./.test(file));
  checkUiTimers(files.map(file => [file, sourceAt(root, file, revision)]));
  console.log('Frontend timers match their documented owners.');
  process.exit(0);
}

const pinnedInventory = resolve(root, 'docs/reactive/refresh-sites.json');
const outputOption = process.argv.find(arg => arg.startsWith('--output='));
const output = outputOption ? resolve(root, outputOption.slice('--output='.length)) : pinnedInventory;
if (process.argv.includes('--check-dispositions')) {
  const sites = JSON.parse(readFileSync(pinnedInventory, 'utf8'));
  const pending = new Set(sites.map(site => `${site.file}:${site.line}`));
  if (pending.size !== sites.length) throw new Error('Duplicate sites in the pinned timing inventory.');
  const classified = new Set();
  for (const document of ['native-timing-dispositions.md', 'ui-refresh-classification.md']) {
    const lines = readFileSync(resolve(root, 'docs/reactive', document), 'utf8').split(/\r?\n/);
    for (const line of lines) {
      if (!line.startsWith('| `')) continue;
      const column = line.split('|')[1];
      for (const [, path, numbers] of column.matchAll(/`([^:`]+):([\d,]+)`/g)) {
        for (const number of numbers.split(',')) {
          const exact = `${path}:${number}`;
          const site = pending.has(exact) || classified.has(exact) ? exact : `src/lib/${exact}`;
          if (classified.has(site)) throw new Error(`Duplicate timing disposition for ${site}`);
          if (!pending.delete(site)) throw new Error(`Timing disposition does not match the pinned inventory: ${exact}`);
          classified.add(site);
        }
      }
    }
  }
  if (pending.size) throw new Error(`Missing timing dispositions:\n${[...pending].join('\n')}`);
  console.log(`All ${sites.length} pinned timing sites have exactly one disposition.`);
  process.exit(0);
}
const trees = ['src/', 'src-tauri/src/', 'crates/', 'apps/', 'packages/', 'scripts/native-gateway-bridge.mjs'];
const excluded = /(?:^|\/)(?:tests?|third_party|vendor|dist|target|fixtures|simulation)(?:\/|$)|(?:[._-](?:tests?|fixtures)\.|\/tests?\.rs$)/;
const timing = /\b(?:setInterval|setTimeout|requestAnimationFrame|recv_timeout|recv_deadline|wait_timeout|park_timeout|sleep|sleep_until|sleep_ms|sleep_us|vTaskDelay|xTaskDelayUntil)\b/g;
const sites = [];
const revisions = [];
for (const repository of ['.', '../device-gateway', '../device-template', '../firmware-common']) {
  const cwd = resolve(root, repository);
  let files, revision;
  try {
    revision = revisionAt(cwd);
    files = filesFromGit(cwd, revision);
  } catch (error) {
    throw new Error(`Cannot inventory ${cwd}: ${error.message}`);
  }
  if (revision) revisions.push(`${repository} ${revision}`);
  for (const file of [...new Set(files)].sort()) {
    if (!/\.(?:svelte|ts|mjs|rs|c|h|cpp|lua)$/.test(file) || excluded.test(file)) continue;
    if (repository === '.' && !trees.some(tree => file.startsWith(tree))) continue;
    const path = resolve(cwd, file);
    const source = sourceAt(cwd, file, revision);
    const lines = source.split(/\r?\n/);
    for (let index = 0; index < lines.length; index++) {
      const line = lines[index];
      if (/^\s*(?:\/\/|\*|#)/.test(line)) continue;
      const kinds = [...new Set(line.match(timing) ?? [])];
      if (!kinds.length) continue;
      sites.push({file:relative(root, path).replaceAll('\\','/'), line:index + 1, kinds});
    }
  }
}
sites.sort((a,b) => a.file.localeCompare(b.file, 'en') || a.line - b.line);
const text = '[\n' + sites.map(site => '  ' + JSON.stringify(site)).join(',\n') + '\n]\n';
if (check) {
  if (readFileSync(output, 'utf8').replaceAll('\r\n','\n') !== text) {
    throw new Error(`Timing-site inventory changed. Run node scripts/reactive/audit.mjs${committed ? ' --committed' : ''}${outputOption ? ` ${outputOption}` : ''} and classify the diff.`);
  }
} else writeFileSync(output, text);
console.log(`${sites.length} timing sites inventoried across Cockpit and firmware repositories.`);
for (const revision of revisions) console.log(revision);
