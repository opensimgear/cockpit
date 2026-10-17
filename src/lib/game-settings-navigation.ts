import { supportedGames, type GameId } from './games';

export const OPEN_GAME_SETTINGS_EVENT = 'osg:open-game-settings';
export const RETURN_FROM_GAME_SETTINGS_EVENT = 'osg:return-from-game-settings';
export type GameSettingsLink<T = unknown> = { gameId: GameId; returnState?: T };
export type GameSettingsVisit<T> = { gameId: GameId; origin: T };
const isGame = (value: unknown): value is GameId => supportedGames.some(game => game.id === value);

/** Local navigation data only. The origin stays owned by its editor/store. */
export function visitGameSettings<T>(gameId: GameId, origin: T): GameSettingsVisit<T> {
  if (!isGame(gameId)) throw new Error('Choose a supported game.');
  return { gameId, origin };
}
export function gameSettingsHref(gameId: GameId): string {
  if (!isGame(gameId)) throw new Error('Choose a supported game.');
  return `#games/${gameId}/integration`;
}
export function gameSettingsFromHash(hash: string): GameId | null {
  const match = /^#games\/([^/]+)\/integration$/.exec(hash);
  return match && isGame(match[1]) ? match[1] : null;
}
export function gameSettingsLink(value: unknown): GameSettingsLink | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const link = value as Record<string, unknown>;
  if (!isGame(link.gameId) || Object.keys(link).some(key => key !== 'gameId' && key !== 'returnState')) return null;
  return { gameId: link.gameId, ...('returnState' in link ? { returnState: link.returnState } : {}) };
}
export function returnFromGameSettings<T>(visit: GameSettingsVisit<T>): T { return visit.origin; }
