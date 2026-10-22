import assert from 'node:assert/strict';
import test from 'node:test';
import { companionMessages } from './companion-messages.ts';

test('conflicting renderer reports surface without claiming a panel or transport',()=>{
  assert.deepEqual(companionMessages({}, {renderProfileError:'Active connections report conflicting renderer profiles.',connectionStatus:'Loading panel'}),[
    {source:'connection',kind:'error',message:'Active connections report conflicting renderer profiles.'},
  ]);
});

test('portable panel diagnostics identify the instance layer and exact target budget',()=>{
  const operation={instanceId:'panel',publicationHash:'hash',phase:'failed',errorCode:'panel-budget',retryable:false,message:'Panel font limit',diagnostic:{widget:'master-widget',definition:'button',layer:'caption',field:'fontSize',required:128,available:48}};
  const message=companionMessages({}, {operation})[0];
  assert.equal(message.kind,'error');assert.match(message.message,/master-widget \/ caption \/ fontSize: 128 required, 48 available/);
  operation.diagnostic.widget='';assert.match(companionMessages({}, {operation})[0].message,/button \/ caption/);
});

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

test('panel preparation replaces inline status with one progress message for USB and network companions', () => {
  const operation={instanceId:'instance',publicationHash:'hash',phase:'downloading',message:'Loading images 50%',completedBytes:65536,totalBytes:131072,cacheHits:2,cacheMisses:1};
  const client={companionId:'screen',connectionStatus:'Opening panel',operation};
  assert.deepEqual(companionMessages({},client),[
    {source:'connection',kind:'info',message:'Preparing panel images · 50% (64 / 128 KiB) · 2 cached, 1 to download'},
  ]);
  const state={host:{instances:[{companionId:'screen',instanceId:'instance',publicationHash:'hash'}]},usb:{connected:true,device:{companionId:'screen',operation}}};
  assert.deepEqual(companionMessages(state,{...client,operation:undefined}),companionMessages({},client));
  state.host.instances[0].publicationHash='new-hash';
  assert.deepEqual(companionMessages(state,{...client,operation:undefined}),[{source:'connection',kind:'info',message:'Opening panel'}]);
});

test('structured panel failures preserve action and error code without duplicating panel errors', () => {
  const client={companionId:'screen',panelError:'Old generic error',connectionStatus:'Old error',operation:{phase:'failed',message:'Active and incoming images do not fit in flash',errorCode:'cache_full',retryable:false}};
  assert.deepEqual(companionMessages({},client),[{source:'connection',kind:'error',message:'Active and incoming images do not fit in flash (cache_full). Check the panel and companion before retrying.'}]);
  client.operation={phase:'failed',message:'Panel image download timed out',errorCode:'image-download-timeout',retryable:true};
  assert.deepEqual(companionMessages({},client),[{source:'connection',kind:'error',message:'Panel image download timed out (image-download-timeout). Check the connection and retry.'}]);
});

test('automatic USB recovery replaces retained failures with one actionable status tooltip', () => {
  const client={companionId:'screen',connectionStatus:'Heartbeat failed',panelError:'Old error',operation:{phase:'failed',errorCode:'request-failed',retryable:true,message:'Invalid USB reply'}};
  for(const phase of ['waiting','connecting','waiting-for-device']){
    const device={connected:phase!=='waiting-for-device',ready:phase!=='waiting-for-device',device:{companionId:'screen',status:'error'},recovery:{phase,attempt:2,nextRetryMs:3000,message:'USB connection interrupted; reconnecting automatically'}};
    for(const state of [{usb:device},{usb:{connected:false,devices:[device]}}]){
      assert.deepEqual(companionMessages(state,client),[{source:'connection',kind:'warning',message:device.recovery.message}]);
      assert.deepEqual(companionMessages(state,{companionId:'other',connectionStatus:'Another companion'}),[{source:'connection',kind:'info',message:'Another companion'}]);
    }
  }
});

test('a recovered live USB port replaces stale recovery history with normal panel feedback', () => {
  const old={connected:false,ready:false,device:{companionId:'screen',status:'error'},recovery:{phase:'waiting-for-device',message:'Reconnecting automatically'}};
  const live={connected:true,ready:true,device:{companionId:'screen',status:'live-armed'},recovery:{phase:'recovered',message:'Recovered old connection'}};
  const client={companionId:'screen',connectionStatus:'Panel connected',operation:{phase:'ready'}};
  assert.deepEqual(companionMessages({usb:{connected:false,devices:[old,live]}},client),[{source:'connection',kind:'info',message:'Panel connected'}]);
  assert.deepEqual(companionMessages({usb:live},client),[{source:'connection',kind:'info',message:'Panel connected'}]);
});
