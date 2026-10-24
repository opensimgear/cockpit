import test from 'node:test';
import assert from 'node:assert/strict';
import { evaluateDailyGate } from './daily-gate.mjs';

test('idle daily run succeeds without selection or native jobs', () => {
  assert.deepEqual(evaluateDailyGate({ hasChanges: false, flags: {}, results: { baseline: 'success', selection: 'skipped' } }), {
    success: true, failures: [],
  });
});

test('selected jobs must complete successfully', () => {
  const report = evaluateDailyGate({
    hasChanges: true,
    flags: { native_host: true, native_mobile: false, native_windows_package: true },
    results: { baseline: 'success', selection: 'success', native: 'failure', mobile: 'skipped', esp32: 'skipped', windows_package: 'success' },
  });
  assert.equal(report.success, false);
  assert.match(report.failures.join(' '), /native job was failure/);
});

test('canceled selected platform cannot be greened over by other jobs', () => {
  const report = evaluateDailyGate({
    hasChanges: true,
    flags: { native_mobile: true },
    results: { baseline: 'success', selection: 'success', native: 'skipped', mobile: 'cancelled', esp32: 'skipped', windows_package: 'skipped' },
  });
  assert.equal(report.success, false);
  assert.match(report.failures.join(' '), /mobile job was cancelled/);
});

test('explicitly deferred ESP32 jobs may remain skipped but failures are surfaced', () => {
  const deferred = evaluateDailyGate({
    hasChanges: true,
    flags: { native_esp32: 'true' },
    results: { baseline: 'success', selection: 'success', native: 'skipped', mobile: 'skipped', esp32: 'skipped', windows_package: 'skipped' },
  });
  assert.equal(deferred.success, true);

  const failed = evaluateDailyGate({
    hasChanges: true,
    flags: { native_esp32: 'true' },
    results: { baseline: 'success', selection: 'success', native: 'skipped', mobile: 'skipped', esp32: 'failure', windows_package: 'skipped' },
  });
  assert.equal(failed.success, false);
  assert.match(failed.failures.join(' '), /esp32 job was failure/);
});
