import assert from 'node:assert/strict';
import test from 'node:test';
import { companionMessages } from './companion-messages.ts';

test('panel failure is an error with its complete native text and replaces the duplicate connection status', () => {
  const detail = 'Panel image could not be loaded · checksum mismatch\nReconnect USB to retry.';
  const state = { firmwareVersion: '2.0', firmware: { screen: { version: '1.0' } } };
  assert.deepEqual(companionMessages(state, {
    companionId: 'screen', kind: 'esp32', panelError: detail, connectionStatus: detail,
  }), [
    { source: 'connection', kind: 'error', message: `Panel error. ${detail}` },
    { source: 'firmware', kind: 'warning', message: 'Firmware update available' },
  ]);
});

test('ordinary connection details are informative and keep their text intact', () => {
  const detail = 'Waiting for panel · USB ready (COM5)';
  assert.deepEqual(companionMessages({}, { companionId: 'screen', connectionStatus: detail }), [
    { source: 'connection', kind: 'info', message: detail },
  ]);
  for (const status of ['', 'No panel assigned']) {
    assert.deepEqual(companionMessages({}, { companionId: 'screen', connectionStatus: status }), []);
  }
});

test('matching USB error and offline states set the connection severity for both inventory shapes', () => {
  const client = { companionId: 'screen', connectionStatus: 'Connection needs attention' };
  for (const [status, kind] of [['error', 'error'], ['offline', 'warning'], ['ready', 'info']]) {
    const usb = { connected: status === 'ready', device: { companionId: 'screen', status } };
    for (const state of [{ usb }, { usb: { connected: false, devices: [usb] } }]) {
      assert.deepEqual(companionMessages(state, client), [
        { source: 'connection', kind, message: client.connectionStatus },
      ]);
    }
  }
});

test('USB and firmware messages are isolated by exact companion identity', () => {
  const state = {
    firmwareVersion: '2.0',
    firmware: { screen: { version: '2.0' }, 'screen-other': { version: '1.0' } },
    usb: {
      connected: false,
      device: { companionId: 'unrelated', status: 'offline', name: 'Same display name' },
      devices: [{ connected: false, device: { companionId: 'screen-other', status: 'error', name: 'Same display name' } }],
    },
  };
  const client = { companionId: 'screen', kind: 'esp32', name: 'Same display name', connectionStatus: 'Waiting for panel' };
  assert.deepEqual(companionMessages(state, client), [
    { source: 'connection', kind: 'info', message: 'Waiting for panel' },
  ]);
  assert.deepEqual(companionMessages(state, { ...client, companionId: 'screen-other' }), [
    { source: 'connection', kind: 'error', message: 'Waiting for panel' },
    { source: 'firmware', kind: 'warning', message: 'Firmware update available' },
  ]);
});

test('ESP32 firmware is quiet when current and informative when its saved version is unknown', () => {
  const client = { companionId: 'screen', kind: 'esp32' };
  assert.deepEqual(companionMessages({ firmwareVersion: '2.0', firmware: { screen: { version: '2.0' } } }, client), []);
  for (const state of [{}, { firmwareVersion: '2.0', firmware: {} }]) {
    assert.deepEqual(companionMessages(state, client), [
      { source: 'firmware', kind: 'info', message: 'Firmware version unknown · connect USB to check' },
    ]);
  }
});

test('browser, mobile and unspecified companion kinds never report ESP32 firmware messages', () => {
  for (const kind of ['browser', 'desktop', 'mobile', undefined]) {
    const client = { companionId: 'screen', kind };
    assert.deepEqual(companionMessages({}, client), []);
    assert.deepEqual(companionMessages({ firmwareVersion: '2.0', firmware: { screen: { version: '1.0' } } }, client), []);
  }
});

test('deriving messages leaves shared host and client snapshots unchanged and returns independent results', () => {
  const state = {
    firmwareVersion: '2.0', firmware: { screen: { version: '1.0', profile: 'sunton-3248s035r' } },
    usb: { connected: false, devices: [{ connected: false, device: { companionId: 'screen', status: 'offline' } }] },
  };
  const client = { companionId: 'screen', kind: 'esp32', connectionStatus: 'USB disconnected' };
  const before = structuredClone({ state, client });
  const freeze = value => { for (const child of Object.values(value)) if (child && typeof child === 'object') freeze(child); return Object.freeze(value); };
  freeze(state); freeze(client);
  const messages = companionMessages(state, client);
  const expected = structuredClone(messages);
  messages[0].message = 'Changed by a caller';
  messages.pop();
  assert.deepEqual(companionMessages(state, client), expected);
  assert.deepEqual({ state, client }, before);
});
