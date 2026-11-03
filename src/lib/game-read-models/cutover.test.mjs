import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {load} from './test-fixture.mjs';

const source=async path=>readFile(new URL(path,import.meta.url),'utf8');

test('game catalogue consumers no longer poll game_get_state',async()=>{
  const consumers=[
    ['application','../../App.svelte'],
    ['games panel','../GamesPanel.svelte'],
    ['profiles panel','../ProfilesPanel.svelte'],
    ['system inventory','../system-inventory-providers.ts'],
  ];
  for(const [name,path] of consumers)assert.doesNotMatch(await source(path),/game_get_state/,`${name} must consume the shared game stream instead of polling`);
  assert.match(await source('../../App.svelte'),/observeGamesState/);
  assert.match(await source('../GamesPanel.svelte'),/modelState/);
  assert.match(await source('../ProfilesPanel.svelte'),/observeGamesState[\s\S]*observeGameDetails/);
  assert.match(await source('../system-inventory-providers.ts'),/observeGamesState/);
});

test('observed catalogue ownership blocks command receipts while direct updates remain available',async()=>{
  const games=await load('../games');
  const catalogue=id=>[{id,name:id,installed:true,launchSupported:true}];
  games.updateGameCatalogue(catalogue('baseline'));
  assert.equal(games.getGameCatalogue()[0].id,'baseline');
  const release=games.claimObservedGameCatalogue();
  try{
    games.updateCommandGameCatalogue(catalogue('stale-command'));
    assert.equal(games.getGameCatalogue()[0].id,'baseline');
    games.updateGameCatalogue(catalogue('manual'));
    assert.equal(games.getGameCatalogue()[0].id,'manual');
  }finally{release();}
  games.updateCommandGameCatalogue(catalogue('command-after-release'));
  assert.equal(games.getGameCatalogue()[0].id,'command-after-release');
});
