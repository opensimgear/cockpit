import test from 'node:test';
import assert from 'node:assert/strict';
import {physicalDeviceIdentity} from './device-identity.ts';

const device = (extra = {}) => ({resource: {kind: 'device'}, assignment: {sourceId: 'right-vkb'}, ...extra});

test('assigned physical blocks identify the exact observed device independently of the profile alias', () => {
  const node = device({resource: {kind: 'device', name: 'Cyclic'}, observation: {id: 'right-vkb', name: ' VKBsim Gladiator EVO R '}});
  assert.equal(physicalDeviceIdentity(node), 'VKBsim Gladiator EVO R');
  assert.equal(node.resource.name, 'Cyclic');
});

test('offline observations retain the known device name', () => {
  assert.equal(physicalDeviceIdentity(device({observation: {id: 'right-vkb', name: 'VKB right', availability: 'offline'}})), 'VKB right');
});

test('a missing assignment cannot reuse an old observed device', () => {
  assert.equal(physicalDeviceIdentity(device({assignment: undefined, observation: {id: 'right-vkb', name: 'Old device'}})), 'Unassigned');
});

test('pending inventory and confirmed missing hardware have distinct labels', () => {
  assert.equal(physicalDeviceIdentity(device(), false), 'Checking device…');
  assert.equal(physicalDeviceIdentity(device()), 'Offline device');
});

test('a different or blank device observation is not substituted', () => {
  assert.equal(physicalDeviceIdentity(device({observation: {id: 'left-vkb', name: 'Wrong device'}})), 'Offline device');
  assert.equal(physicalDeviceIdentity(device({observation: {id: 'right-vkb', name: '  '}}), false), 'Checking device…');
});

test('other block kinds keep their existing headings', () => {
  for (const kind of ['game', 'companion', 'debug-controller', undefined]) {
    assert.equal(physicalDeviceIdentity({resource: kind ? {kind} : undefined}), undefined);
  }
});
