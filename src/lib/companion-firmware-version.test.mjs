import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';
const {companionFirmwareVersion:version,firmwareUpdateAvailable:update}=(await loadModules(['./companion-firmware'])).get('./companion-firmware');
const state=()=>({firmwareVersion:null,firmware:{small:{version:'2026.10.21.1',profile:'sunton'},large:{version:'2026.10.19.8',profile:'waveshare'}},firmwareProfiles:[{id:'sunton',available:true,version:'2026.10.22.3'},{id:'waveshare',available:true,version:'2026.10.21.1'}]});
test('firmware updates use the selected board when staged versions differ',()=>{const s=state();assert.equal(version(s,'small'),'2026.10.22.3');assert.equal(version(s,'large'),'2026.10.21.1');assert.equal(update(s.firmware.small.version,version(s,'small')),true);});
test('unavailable and unknown boards never borrow the global release version',()=>{const s=state();s.firmwareVersion='9.0.0';s.firmwareProfiles[0].available=false;assert.equal(version(s,'small'),null);assert.equal(version(s,'missing'),null);s.firmwareProfiles[0].available=true;delete s.firmwareProfiles[0].version;assert.equal(version(s,'small'),null);});
test('older hosts without per-board metadata retain their shared release version',()=>{assert.equal(version({firmwareVersion:'1.2.3'},'screen'),'1.2.3');assert.equal(version({firmwareProfiles:[]},'screen'),null);});
