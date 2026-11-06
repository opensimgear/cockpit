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
function* sourcesAt(cwd, files, revision) {
  if (!revision) {
    for (const file of files) yield [file, readFileSync(resolve(cwd, file), 'utf8')];
    return;
  }
  // One Git process per repository keeps the audit usable on Windows.
  // Parse the batch by byte length so UTF-8 and newlines in blobs stay intact.
  const output = execFileSync('git', ['cat-file', '--batch'], {
    cwd,
    input: files.map(file => `${revision}:${file}\n`).join(''),
    maxBuffer: 128 * 1024 * 1024,
  });
  let offset = 0;
  for (const file of files) {
    const headerEnd = output.indexOf(10, offset);
    const header = headerEnd < 0 ? '' : output.toString('ascii', offset, headerEnd);
    const match = /^[a-f0-9]+ blob (\d+)$/.exec(header);
    if (!match) throw new Error(`Cannot read committed source ${file}: ${header}`);
    const start = headerEnd + 1;
    const end = start + Number(match[1]);
    if (output[end] !== 10) throw new Error(`Incomplete committed source ${file}`);
    yield [file, output.toString('utf8', start, end)];
    offset = end + 1;
  }
}

if (process.argv.includes('--check-ui-timers')) {
  const { checkUiTimers } = await import('./ui-timers.mjs');
  const revision = revisionAt(root);
  const files = [...new Set(filesFromGit(root, revision))].filter(file =>
    /^(?:src|apps|packages)\//.test(file) && /\.(?:[cm]?[jt]s|[jt]sx|svelte)$/.test(file)
    && !/(?:^|\/)(?:tests?|fixtures|dist|target|vendor|simulation)(?:\/|$)|(?:^|\/)test-fixtures?\.|[._-](?:tests?|fixtures)\./.test(file));
  checkUiTimers(sourcesAt(root, files, revision));
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
// Event waits can carry a periodic fallback or an exact deadline. Inventory
// the call sites as well as the lower-level wait that implements them.
const eventWait = /\b(wait_scheduled|wait_deadline|wait_until|WaitForSingleObject(?:Ex)?|(?:Msg)?WaitForMultipleObjects(?:Ex)?|set_read_timeout|set_write_timeout|xQueueReceive|ulTaskNotifyTake|xTaskNotifyWait|tud_task|tight_loop_contents)\s*\(/g;
const nativeReadiness = {
  rust: /\b(poll\.poll)\s*\(/g,
  c: /\b(select|poll|ppoll|epoll_wait|kevent)\s*\(/g,
};
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
  const productionFiles = [...new Set(files)].sort().filter(file =>
    /\.(?:svelte|ts|mjs|rs|c|h|cpp|lua)$/.test(file)
      && (!excluded.test(file) || (repository === '../device-gateway' && file.startsWith('simulation/pico/firmware/src/')))
      && (repository !== '.' || trees.some(tree => file.startsWith(tree))));
  for (const [file, source] of sourcesAt(cwd, productionFiles, revision)) {
    const path = resolve(cwd, file);
    const readiness = file.endsWith('.rs') ? nativeReadiness.rust : /\.(?:c|h|cpp)$/.test(file) ? nativeReadiness.c : undefined;
    const lines = source.split(/\r?\n/);
    for (let index = 0; index < lines.length; index++) {
      const line = lines[index];
      if (/^\s*(?:\/\/|\*|#)/.test(line)) continue;
      const kinds = [...new Set([
        ...(line.match(timing) ?? []),
        ...Array.from(line.matchAll(eventWait), match => match[1]),
        ...(readiness ? Array.from(line.matchAll(readiness), match => match[1]) : []),
      ])];
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
