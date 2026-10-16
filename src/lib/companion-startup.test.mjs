import test from 'node:test';
import assert from 'node:assert/strict';
import { restoreAssignedPanels } from './companion-startup.ts';
test('restored native panels need no frontend library access',async()=>{
 await restoreAssignedPanels({host:{instances:[{publicationHash:'a'}],publications:[{publicationHash:'a'}]}},()=>{throw Error('unexpected load');},()=>{throw Error('unexpected publish');});
});
test('legacy startup restores only missing assignments, once, before completion',async()=>{
 const sent=[];const a={publicationHash:'a'},b={publicationHash:'b'},unused={publicationHash:'unused'};
 await restoreAssignedPanels({host:{instances:[a,b,b],publications:[a]}},async()=>[a,b,unused],async panel=>{await Promise.resolve();sent.push(panel.publicationHash);});
 assert.deepEqual(sent,['b']);
});
test('missing saved content reports actionable error before publishing anything',async()=>{
 const sent=[];
 await assert.rejects(restoreAssignedPanels({host:{instances:[{publicationHash:'a'},{publicationHash:'missing'}]}},async()=>[{publicationHash:'a'}],async p=>sent.push(p)),/select a saved panel/);
 assert.deepEqual(sent,[]);
});
