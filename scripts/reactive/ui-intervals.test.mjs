import assert from 'node:assert/strict';
import test from 'node:test';
import { checkUiIntervals } from './ui-intervals.mjs';

test('frontend interval guard detects multiline direct and property calls', () => {
  for (const callee of ['setInterval', 'window.setInterval', 'window["setInterval"]']) {
    assert.throws(() => checkUiIntervals([['src/Test.svelte',
      `<script lang="ts">const timer = ${callee}\n(() => fetch('/status'), 1000);</script>`]]),
    /unclassified interval/);
  }
  assert.doesNotThrow(() => checkUiIntervals([['src/Deadline.ts',
    'const reminder = setTimeout(expire, 1000); const label = "setInterval(fetch, 1)";']]));
});

test('an approved callback cannot gain a refresh or a second interval', () => {
  const file = 'apps/browser-companion/src/connection-worker.ts';
  const allowed = 'timer = setInterval(() => void connection.tick().catch(stop), 250);';
  assert.doesNotThrow(() => checkUiIntervals([[file, allowed]]));
  assert.throws(() => checkUiIntervals([[file, allowed + allowed]]), /unclassified interval/);
  assert.throws(() => checkUiIntervals([[file, allowed.replace('connection.tick()', 'fetch("/status")')]]),
    /unclassified interval/);
});

test('dormant timers cannot silently gain a production caller', () => {
  assert.throws(() => checkUiIntervals([['src/App.svelte',
    '<script>import Desktop from "./lib/ProfileDesktopControls.svelte";</script>']]),
  /dormant ProfileDesktopControls has a production reference/);
});
