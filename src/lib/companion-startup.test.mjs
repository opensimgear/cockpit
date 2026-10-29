import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {readNativeCompanionState} from './companion-startup.ts';

test('native retained state is observed without replaying any frontend archive or head',async()=>{
  const state={host:{instances:[{publicationHash:'assigned'}],publications:[{publicationHash:'assigned'},{publicationHash:'saved'}]},panelSaved:{panelId:'panel',publicationHash:'saved'}};
  const commands=[];assert.equal(await readNativeCompanionState(async command=>{commands.push(command);return state;}),state);assert.deepEqual(commands,['companion_status']);
});
test('missing content is left with its native owner instead of reconstructed implicitly',async()=>{
  const state={host:{instances:[{publicationHash:'missing'}],publications:[]}},commands=[];
  assert.equal(await readNativeCompanionState(async command=>{commands.push(command);return state;}),state);assert.deepEqual(commands,['companion_status']);assert.deepEqual(state.host.publications,[]);
});
test('native read failures propagate without falling back to a designer or browser cache',async()=>{
  await assert.rejects(readNativeCompanionState(async()=>{throw Error('SQLite unavailable');}),/SQLite unavailable/);
  await assert.rejects(readNativeCompanionState(async()=>({})),/native Cockpit/);
});
test('mounting the frontend leaves native startup trust and publications with their native owner',async()=>{
  const source=await readFile(new URL('../App.svelte',import.meta.url),'utf8');assert.equal(source.includes('restoreCompanionPanels'),false);assert.equal(source.includes("'companion_status'"),false);
});
