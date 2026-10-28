import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./games', './profile-game-source']);
const { gameIntegrationSourceChoices, gameIntegrationSourceStatus, matchingGameIntegrationSources } = modules.get('./profile-game-source');
const game = (id, provider = id, integrationKey = id) => ({ id, name: id, provider, integrationKey });
const source = (attributes = {}) => ({ resourceType: 'game-integration', attributes: { gameId: 'dcs', integrationKey: 'dcs', installed: 'true', setupStatus: 'ready', gameContextStatus: 'waiting-game', processVerified: 'false', ...attributes } });

test('game source status labels use the current native integration attributes', () => {
  const dcs = game('dcs');
  assert.equal(gameIntegrationSourceStatus(dcs, [source({ setupStatus: 'setup-required' })]), 'Configuration needed');
  assert.equal(gameIntegrationSourceStatus(dcs, [source({ gameContextStatus: 'ready', processVerified: 'true' })]), 'Running');
  assert.equal(gameIntegrationSourceStatus(dcs, [source({ gameContextStatus: 'waiting-game', processVerified: 'false' })]), 'Not running');
  assert.equal(gameIntegrationSourceStatus(dcs, [source({ installed: 'false', setupStatus: 'not-installed' })]), 'Missing');
});

test('current Games catalogue setup and process state override stale resource attributes', () => {
  const dcs = { ...game('dcs'), installed: true, integrationSetup: { state: 'ready' }, integrationContext: { processVerified: true } };
  assert.equal(gameIntegrationSourceStatus(dcs, [source({ setupStatus: 'setup-required', processVerified: 'false' })]), 'Running');
  assert.equal(gameIntegrationSourceStatus({ ...dcs, integrationSetup: { state: 'setup-required' } }, [source({ setupStatus: 'ready', processVerified: 'true' })]), 'Configuration needed');
  assert.equal(gameIntegrationSourceStatus({ ...dcs, installed: false }, []), 'Missing');
});

test('game source choices require a provider and match the exact native integration identity', () => {
  const games = [game('dcs'), game('x-plane-12', 'x-plane', 'x-plane-12'), game('plain-game', null)];
  const choices = gameIntegrationSourceChoices(games, [
    source({ gameId: 'dcs', integrationKey: 'wrong' }),
    source({ gameId: 'x-plane-12', integrationKey: 'x-plane-12', setupStatus: 'setup-required' }),
  ]);
  assert.deepEqual(choices.map(choice => [choice.game.id, choice.status]), [
    ['dcs', 'Configuration needed'],
    ['x-plane-12', 'Configuration needed'],
  ]);
  const exact = source({ gameId: 'x-plane-12', integrationKey: 'x-plane-12' });
  assert.deepEqual(matchingGameIntegrationSources(games[1], [source(), exact]), [exact]);
  assert.equal(matchingGameIntegrationSources(games[2], [exact]).length, 0);
});
