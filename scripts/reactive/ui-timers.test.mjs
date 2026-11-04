import assert from 'node:assert/strict';
import test from 'node:test';
import { checkUiTimers } from './ui-timers.mjs';

test('frontend interval guard detects multiline direct and property calls', () => {
  for (const callee of ['setInterval', 'window.setInterval', 'window["setInterval"]']) {
    assert.throws(() => checkUiTimers([['src/Test.svelte',
      `<script lang="ts">const timer = ${callee}\n(() => fetch('/status'), 1000);</script>`]]),
    /unclassified setInterval/);
  }
  assert.doesNotThrow(() => checkUiTimers([['src/Deadline.ts',
    'const label = "setInterval(fetch, 1)";']]));
});

test('an approved deadline cannot be replaced or duplicated', () => {
  const file = 'apps/browser-companion/src/connection-worker.ts';
  const allowed = 'timer = setTimeout(maintain, Math.max(0, Math.ceil(next - performance.now())));';
  assert.doesNotThrow(() => checkUiTimers([[file, allowed + 'setTimeout(resolve, 1000);']]));
  assert.throws(() => checkUiTimers([[file, allowed + allowed]]), /unclassified setTimeout/);
  assert.throws(() => checkUiTimers([[file, allowed.replace('maintain', '() => fetch("/status")')]]),
    /unclassified setTimeout/);
});

test('a self-scheduling timeout requires an explicit owner', () => {
  for (const callee of ['setTimeout', 'window.setTimeout', 'globalThis["setTimeout"]', 'globalThis[`setTimeout`]']) {
    assert.throws(() => checkUiTimers([['src/Repeat.ts',
      `async function refresh(){ await fetch('/status'); ${callee}(refresh, 250); }`]]),
    /unclassified setTimeout/);
  }
});

test('whitespace in timer arguments is ignored only outside string literals', () => {
  const file = 'apps/browser-companion/src/socket-transport.ts';
  const connect = "setTimeout(() => { socket.close(); reject(new Error('Cockpit connection timed out')); }, 3000);";
  const reply = "setTimeout(() => { this.fail(new Error('Cockpit reply timed out')); this.socket?.close(); }, 4000);";
  assert.doesNotThrow(() => checkUiTimers([[file, connect + reply]]));
  assert.throws(() => checkUiTimers([[file, connect.replace('connection timed', 'connectiontimed') + reply]]),
    /unclassified setTimeout/);
  assert.throws(() => checkUiTimers([[file, connect]]), /stale setTimeout/);
});

test('dormant timers cannot silently gain a production caller', () => {
  assert.throws(() => checkUiTimers([['src/App.svelte',
    '<script>import Desktop from "./lib/ProfileDesktopControls.svelte";</script>']]),
  /dormant ProfileDesktopControls has a production reference/);
});
