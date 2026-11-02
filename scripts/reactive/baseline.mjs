// Read-only native Cockpit baseline. Does not activate or recover a profile.
import {execFileSync} from 'node:child_process';
import {mkdir, writeFile, readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';

const endpoint = process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222';
const pages = await (await fetch(`${endpoint}/json`)).json();
const page = pages.find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url));
if (!page) throw new Error('Start the native dev Cockpit before collecting the baseline.');
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve,reject) => {socket.onopen=resolve;socket.onerror=reject;});
let next = 0;
const pending = new Map();
const calls = [];
socket.onmessage = event => {
  const reply = JSON.parse(event.data);
  if (reply.method === 'Network.requestWillBeSent') {
    const {request,timestamp} = reply.params;
    if (/gateway_request/.test(request.url)) {
      let command = 'unparsed';
      try {command = JSON.parse(request.postData).request.command;} catch {}
      calls.push({command,atSeconds:timestamp});
    }
  }
  const request = pending.get(reply.id);
  if (!request) return;
  pending.delete(reply.id); clearTimeout(request.timer);
  if (reply.error) request.reject(new Error(JSON.stringify(reply.error)));
  else request.resolve(reply.result);
};
const send = async (method,params) => {
  const id = ++next;
  const response = await new Promise((resolve,reject) => {
    const timer = setTimeout(() => {pending.delete(id);reject(new Error('Cockpit inspection timed out'));},10000);
    pending.set(id,{resolve,reject,timer});
    socket.send(JSON.stringify({id,method,params}));
  });
  return response;
};
const evaluate = async expression => {
  const response = await send('Runtime.evaluate',{expression,returnByValue:true});
  if (response.exceptionDetails) throw new Error(JSON.stringify(response.exceptionDetails));
  return response.result.value;
};
const startedAt = new Date().toISOString();
try {
  await send('Network.enable',{});
  await evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'profile_status',clientId:'reactive-baseline-check'}}).catch(()=>{})`);
  console.log('Observing actual native Cockpit gateway traffic for 60 seconds.');
  await new Promise(resolve => setTimeout(resolve,60000));
  const trace = await evaluate(`(() => {
    return {title:document.title,
      visible:!document.hidden,view:document.querySelector('main')?.innerText.slice(0,2000)};
  })()`);
  const counts = {};
  if (!calls.some(call => call.command === 'profile_status')) throw new Error('Instrumentation self-check was not observed');
  for (const call of calls) counts[call.command] = (counts[call.command] ?? 0) + 1;
  const executable = await readFile('src-tauri/target/debug/osg-cockpit.exe');
  const result = {startedAt,head:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),
    dirty:execFileSync('git',['status','--porcelain'],{encoding:'utf8'}).trim().length>0,
    executableSha256:createHash('sha256').update(executable).digest('hex'),
    qualification:'Baseline only; no physical/simulator latency or healthy owner qualification.',
    durationMs:Date.now()-Date.parse(startedAt),counts,calls,...trace};
  await mkdir('.research/reactive',{recursive:true});
  await writeFile('.research/reactive/baseline.json',JSON.stringify(result,null,2)+'\n');
  console.log(JSON.stringify({durationMs:result.durationMs,counts,visible:trace.visible}));
} finally {
  socket.close();
}
