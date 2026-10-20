import test from 'node:test';
import assert from 'node:assert/strict';
import { restoreAssignedPanels,restoreSavedPanelLibrary } from './companion-startup.ts';
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

test('startup restores missing transport content without making it current, then establishes one latest saved state per panel',async()=>{
 const old={publicationHash:'old'},latest={publicationHash:'latest'},another={publicationHash:'another'},removed={publicationHash:'removed'};
 const saved={schemaVersion:1,library:{schemaVersion:1,drafts:[{id:'panel-one'},{id:'panel-two'}]},publications:{'panel-one':[old,latest],'panel-two':[another],'deleted-panel':[removed]}};
 const calls=[];
 await restoreSavedPanelLibrary({host:{instances:[old],publications:[]}},saved,async(panel,options)=>calls.push([panel.publicationHash,options.saveAsLatest]));
 assert.deepEqual(calls,[['old',false],['latest',true],['another',true]]);
});
