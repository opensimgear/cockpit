import test from 'node:test';
import assert from 'node:assert/strict';
import { planNative } from './plan-native.mjs';

test('maps affected marker projects to only their platform jobs', () => {
  assert.deepEqual(planNative({ projects: ['native-host', 'native-mobile'] }), {
    native_host: true,
    native_controllers: false,
    native_profiles: false,
    native_persistence: false,
    native_mapping: false,
    native_logic: false,
    native_mobile: true,
    native_esp32: false,
    native_windows_package: false,
  });
});

test('missing baseline selects every native integration job', () => {
  assert.ok(Object.values(planNative({ projects: [], full: true })).every(Boolean));
});
