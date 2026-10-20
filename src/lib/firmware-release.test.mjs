import assert from 'node:assert/strict';
import test from 'node:test';
import { companionMessages } from './companion-messages.ts';
import { firmwareSupportsImages } from './companion-screen.ts';
import { compareFirmwareVersions, firmwareMeetsAvailable, firmwareUpdateAvailable } from './companion-firmware.ts';

test('unknown or failed release lookups never invent an available firmware update', () => {
  const client={kind:'esp32',companionId:'screen'};
  for(const version of [undefined,null,'']) {
    const state={firmwareVersion:version,firmware:{screen:{version:'2026.10.20.1'}},firmwareError:'No published firmware release'};
    assert.deepEqual(companionMessages(state,client),[]);
  }
  assert.deepEqual(companionMessages({firmwareVersion:'0.2.0',firmware:{screen:{version:'0.1.0'}}},client),[
    {source:'firmware',kind:'warning',message:'Firmware update available'},
  ]);
});

test('newer installed releases stay pairable without being offered a downgrade', () => {
  assert.equal(compareFirmwareVersions('0.3.0','0.2.0'),1);
  assert.equal(firmwareUpdateAvailable('0.3.0','0.2.0'),false);
  assert.equal(firmwareMeetsAvailable('0.3.0','0.2.0'),true);
  assert.equal(firmwareUpdateAvailable('0.2.0','0.3.0'),true);
  assert.equal(firmwareMeetsAvailable('0.2.0','0.3.0'),false);
  assert.equal(firmwareUpdateAvailable('2026.10.20.1','0.1.0'),true);
  assert.equal(firmwareMeetsAvailable('2026.10.20.1','0.1.0'),false);
  assert.equal(firmwareUpdateAvailable('2026.10.20.1','2026.10.19.7'),false);
  assert.equal(firmwareUpdateAvailable('0.3.0',null),false);
  assert.deepEqual(companionMessages({firmwareVersion:'0.2.0',firmware:{screen:{version:'0.3.0'}}},{kind:'esp32',companionId:'screen'}),[]);
});

test('image compatibility accepts component SemVer and preserves legacy firmware floors', () => {
  for(const version of ['0.1.0','0.2.1','1.0.0','0.2.0-rc.1','2026.10.19.7','2026.10.20.1'])
    assert.equal(firmwareSupportsImages(version),true,version);
  for(const version of ['0.0.1','2026.10.19.6','2026.9.1.1','unknown','','01.2.3'])
    assert.equal(firmwareSupportsImages(version),false,version);
});
