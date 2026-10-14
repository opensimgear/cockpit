import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';

export function processClient(executable, args, env) {
  const child = spawn(executable, args, { stdio: ['pipe', 'pipe', 'pipe'], env });
  const pending = [];
  let stderr = '', failure, sequence = 0;
  child.stderr.on('data', bytes => { stderr = (stderr + bytes).slice(-16384); });
  function fail(error) {
    failure = error;
    for (const item of pending.splice(0)) { clearTimeout(item.timer); item.reject(error); }
  }
  createInterface({ input: child.stdout }).on('line', line => {
    let reply;
    try { reply = JSON.parse(line); } catch { fail(new Error(`Invalid process JSON: ${line}`)); return; }
    const index = pending.findIndex(item => item.requestId === reply.requestId);
    if (index < 0) { fail(new Error(`Unexpected process JSON: ${line}`)); return; }
    const [item] = pending.splice(index, 1);
    clearTimeout(item.timer);
    item.resolve(item.requestId === undefined ? reply : reply.response);
  });
  child.once('error', fail);
  child.stdin.on('error', fail);
  child.once('exit', (code, signal) => fail(new Error(`Process exited (${code ?? signal}): ${stderr}`)));
  function send(line, requestId) {
    if (failure) return Promise.reject(failure);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { fail(new Error(`Process timeout: ${line}\n${stderr}`)); child.kill('SIGKILL'); }, 20000);
      pending.push({ resolve, reject, timer, requestId });
      child.stdin.write(`${line}\n`, error => { if (error) fail(error); });
    });
  }
  return {
    child,
    line: line => send(line),
    request(value) {
      const requestId = `radio-${++sequence}`;
      return send(JSON.stringify({ requestId, request: value }), requestId);
    },
    async stop() {
      if (child.exitCode !== null || child.signalCode !== null) return;
      const exited = new Promise(resolve => child.once('exit', resolve));
      child.kill('SIGTERM');
      const timer = setTimeout(() => child.kill('SIGKILL'), 3000);
      await exited; clearTimeout(timer);
      assert.doesNotMatch(stderr, /AddressSanitizer|runtime error:/, 'Native sanitizers remain clean');
    },
  };
}
