// Inventory timing sites, including legitimate leases/deadlines; never a timer ban.
import {execFileSync} from 'node:child_process';
import {readFileSync, writeFileSync} from 'node:fs';
import {dirname, resolve, relative} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const check = process.argv.includes('--check');
const committed = process.argv.includes('--committed');
const filesFromGit = cwd => execFileSync('git', committed
  ? ['ls-tree', '-r', '--name-only', '-z', 'HEAD']
  : ['ls-files', '--cached', '--others', '--exclude-standard', '-z'],
{cwd, encoding:'utf8'}).split('\0');
const sourceAt = (cwd, file) => committed
  ? execFileSync('git', ['show', `HEAD:${file}`], {cwd, encoding:'utf8'})
  : readFileSync(resolve(cwd, file), 'utf8');

if (process.argv.includes('--check-ui-intervals')) {
  const { checkUiIntervals } = await import('./ui-intervals.mjs');
  const files = [...new Set(filesFromGit(root))].filter(file =>
    /^(?:src|apps|packages)\//.test(file) && /\.(?:[cm]?[jt]s|[jt]sx|svelte)$/.test(file)
    && !/(?:^|\/)(?:tests?|fixtures|dist|target|vendor|simulation)(?:\/|$)|[._-](?:tests?|fixtures)\./.test(file));
  checkUiIntervals(files.map(file => [file, sourceAt(root, file)]));
  console.log('Frontend intervals match their documented owners.');
  process.exit(0);
}

const output = resolve(root, 'docs/reactive/refresh-sites.json');
const trees = ['src/', 'src-tauri/src/', 'crates/', 'apps/', 'packages/', 'scripts/native-gateway-bridge.mjs'];
const excluded = /(?:^|\/)(?:tests?|third_party|vendor|dist|target|fixtures|simulation)(?:\/|$)|(?:[._-](?:tests?|fixtures)\.|\/tests?\.rs$)/;
const timing = /\b(?:setInterval|setTimeout|requestAnimationFrame|recv_timeout|recv_deadline|wait_timeout|park_timeout|sleep|sleep_until|sleep_ms|sleep_us|vTaskDelay|xTaskDelayUntil)\b/g;
const sites = [];
for (const repository of ['.', '../device-gateway', '../device-template', '../firmware-common']) {
  const cwd = resolve(root, repository);
  let files;
  try {
    files = filesFromGit(cwd);
  } catch (error) {
    throw new Error(`Cannot inventory ${cwd}: ${error.message}`);
  }
  for (const file of [...new Set(files)].sort()) {
    if (!/\.(?:svelte|ts|mjs|rs|c|h|cpp|lua)$/.test(file) || excluded.test(file)) continue;
    if (repository === '.' && !trees.some(tree => file.startsWith(tree))) continue;
    const path = resolve(cwd, file);
    const source = sourceAt(cwd, file);
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
    throw new Error(`Timing-site inventory changed. Run node scripts/reactive/audit.mjs${committed ? ' --committed' : ''} and classify the diff.`);
  }
} else writeFileSync(output, text);
console.log(`${sites.length} timing sites inventoried across Cockpit and firmware repositories.`);
