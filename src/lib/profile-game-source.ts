import { gameHasIntegration, type Game } from './games';
import type { ResourceSummary } from './profile-inventory';

export type GameIntegrationSourceStatus = 'Running' | 'Not running' | 'Configuration needed' | 'Missing';

/** Status for a game selector comes from the current native integration summary. */
export function gameIntegrationSourceStatus(game: Game | undefined, resources: readonly Pick<ResourceSummary, 'resourceType' | 'attributes'>[]): GameIntegrationSourceStatus {
  if (game?.installed === false) return 'Missing';
  if (game?.integrationSetup) {
    if (game.integrationSetup.state !== 'ready') return 'Configuration needed';
    return game.integrationContext?.processVerified ? 'Running' : 'Not running';
  }
  const source = game
    ? resources.find(resource => resource.resourceType === 'game-integration'
      && resource.attributes.gameId === game.id && resource.attributes.integrationKey === (game.integrationKey ?? game.id))
    : resources.find(resource => resource.resourceType === 'game-integration');
  if (!source) return 'Configuration needed';
  const attributes = source.attributes;
  if (attributes.installed === 'false') return 'Missing';
  if (attributes.setupStatus !== 'ready') return 'Configuration needed';
  return attributes.processVerified === 'true' ? 'Running' : 'Not running';
}

/** Game source choices must belong to the exact integration selected for this graph. */
export function matchingGameIntegrationSources<T extends Pick<ResourceSummary, 'resourceType' | 'attributes'>>(game: Game | undefined, resources: readonly T[]): T[] {
  if (!game || !gameHasIntegration(game)) return [];
  return resources.filter(resource => resource.resourceType === 'game-integration'
    && resource.attributes.gameId === game.id
    && resource.attributes.integrationKey === (game.integrationKey ?? game.id));
}

/** Selectable contexts are integrations actually advertised by the game catalogue. */
export function gameIntegrationSourceChoices(games: readonly Game[], resources: readonly Pick<ResourceSummary, 'resourceType' | 'attributes'>[]) {
  return games.filter(gameHasIntegration).map(game => ({
    game,
    status: gameIntegrationSourceStatus(game, resources),
  }));
}
