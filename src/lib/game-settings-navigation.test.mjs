import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';
const modules = await loadModules(['./runtime-config','./panel-input-metadata','./games','./game-settings-navigation']);
const { supportedGames } = modules.get('./games');
const { gameSettingsFromHash, gameSettingsHref, gameSettingsLink, returnFromGameSettings, visitGameSettings } = modules.get('./game-settings-navigation');

test('each provider has one game-owned settings deep link', () => {
  for (const game of supportedGames) assert.equal(gameSettingsFromHash(gameSettingsHref(game.id)), game.id);
  for (const hash of ['', '#dcs-bios', '#games/other/integration', '#games/dcs', '#games/dcs/integration/extra', '#games/%64cs/integration']) assert.equal(gameSettingsFromHash(hash), null);
  assert.throws(() => gameSettingsHref('other'), /supported game/);
  assert.throws(() => visitGameSettings('other', {}), /supported game/);
});
test('settings navigation preserves the editor store and newer unsaved edits by reference', () => {
  const editor = { document: { name: 'Unsaved profile', bindings: ['existing-edge'] }, appliedRevision: 2 };
  const origin = { view: 'profiles', profileId: 'profile', editor };
  const visit = visitGameSettings('dcs', origin);
  editor.document.name = 'Edit made while settings are open';
  assert.equal(returnFromGameSettings(visit), origin);
  assert.equal(returnFromGameSettings(visit).editor, editor);
  assert.equal(returnFromGameSettings(visit).editor.document.name, 'Edit made while settings are open');
  assert.equal(editor.appliedRevision, 2);
  assert.deepEqual(editor.document.bindings, ['existing-edge']);
});
test('graph links carry only a supported game and opaque return state', () => {
  const store = { draft: 'current draft' };
  const link = gameSettingsLink({ gameId: 'dcs', returnState: store });
  assert.equal(link.returnState, store);
  assert.equal(gameSettingsLink({ gameId: 'x-plane-12' }).gameId, 'x-plane-12');
  for (const invalid of [null, [], false, {}, { gameId: 'other' }, { gameId: 'dcs', apply: true }, { gameId: 'dcs', profileDocument: {} }]) assert.equal(gameSettingsLink(invalid), null);
});
