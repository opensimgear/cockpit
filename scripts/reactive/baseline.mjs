// Read-only native Cockpit baseline. Does not activate or recover a profile.
import {execFileSync} from 'node:child_process';
import {createReadStream} from 'node:fs';
import {mkdir, writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {dirname, resolve} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {nativePage, until} from './cdp.mjs';

const executable = resolve(process.env.OSG_BASELINE_EXECUTABLE ?? 'src-tauri/target/debug/osg-cockpit.exe');
const output = resolve(process.env.OSG_BASELINE_OUTPUT ?? '.research/reactive/baseline.json');
const hash = createHash('sha256');
for await (const chunk of createReadStream(executable)) hash.update(chunk);
const executableSha256 = hash.digest('hex');
const head = execFileSync('git', ['rev-parse', 'HEAD'], {encoding:'utf8'}).trim();
const dirty = execFileSync('git', ['status', '--porcelain'], {encoding:'utf8'}).trim().length > 0;

const clientId = 'reactive-baseline-check';
const page = await nativePage();
const calls = [];
let probes = 0;
let collecting = false;
page.listen(({method, params}) => {
  if (method !== 'Network.requestWillBeSent') return;
  const {request, timestamp} = params;
  if (request.method !== 'POST' || new URL(request.url).pathname !== '/gateway_request') return;
  let gateway;
  try { gateway = JSON.parse(request.postData).request; } catch {}
  if (gateway?.command === 'profile_status' && gateway.clientId === clientId) {
    probes++;
  } else if (collecting) {
    calls.push({command:gateway?.command ?? 'unparsed', atSeconds:timestamp});
  }
});

async function sampleEndpoint(expectedProbes) {
  const sample = await page.evaluate(`(async () => {
    const reply = await window.__TAURI_INTERNALS__.invoke('gateway_request', {
      request:{command:'profile_status',clientId:${JSON.stringify(clientId)}},
    });
    const session = reply.state?.profiles?.profileSession;
    if (!reply.ok || !session?.state) throw new Error('Read-only status probe failed');
    return {
      state:session.state, owner:session.owner ?? null, fault:session.fault ?? null,
      visible:!document.hidden, view:document.querySelector('main h1')?.textContent?.trim() ?? null,
    };
  })()`);
  await until(() => probes, count => count === expectedProbes, 2000);
  return sample;
}

try {
  await until(() => page.evaluate("Boolean(window.__TAURI_INTERNALS__?.invoke && document.querySelector('main'))"));
  await page.call('Network.enable', {maxPostDataSize:65536});
  const before = await sampleEndpoint(1);
  console.log('Observing native Cockpit gateway requests for 60 seconds.');
  const startedAt = new Date().toISOString();
  const started = performance.now();
  collecting = true;
  await delay(60000);
  collecting = false;
  const durationMs = performance.now() - started;
  const after = await sampleEndpoint(2);
  const commands = new Map();
  for (const {command} of calls) commands.set(command, (commands.get(command) ?? 0) + 1);
  const counts = Object.fromEntries(commands);
  const result = {
    startedAt, head, dirty, executable, executableSha256, durationMs, counts, calls, probes,
    before, after,
    qualification:'Main WebView gateway POSTs only. Status samples are endpoint checks.',
  };
  await mkdir(dirname(output), {recursive:true});
  await writeFile(output, JSON.stringify(result, null, 2) + '\n');
  console.log(JSON.stringify({durationMs, counts, probes, before, after, output}));
} finally {
  page.close();
}
