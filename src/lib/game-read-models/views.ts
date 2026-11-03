import type { Game, GamesState } from '../games.ts';
import type { GameDetailModel, GameJobsModel, GameModelState } from './models.ts';
import type { DeepReadonly } from '../profiles.ts';
export function gameDetailMatches(summary: Game | undefined, detail: GameModelState<GameDetailModel>): boolean {
  return !!summary?.detailsRevision && detail.status === 'ready' && detail.value?.gameId === summary.id && detail.value.scope === summary.detailsRevision;
}
/** Retain the selected game's complete cached view while a newer scope is loading. */
export function selectedGameView(summary: Game | undefined, detail: GameModelState<GameDetailModel>): Game | undefined {
  return summary && detail.value?.gameId === summary.id ? detail.value.value as Game : summary;
}
/** Small job receipts can withdraw an old review before the next large tree is decoded. */
export function gamesWithJobs(state: GamesState, model: DeepReadonly<GameJobsModel> | undefined): GamesState {
  const job = model?.jobs.find(value => value.key === 'discovery');
  if (!job || (job.run === state.discovery?.scanId
    && (job.terminal || ['ready', 'applied', 'failed'].includes(state.discovery.status)))) return state;
  const previous = state.discovery, replaced = previous?.scanId !== job.run;
  return { ...state, discovery: {
    scanId: job.run, status: job.phase === 'failed' ? 'failed' : 'scanning', firstStart: previous?.firstStart ?? false,
    libraries: replaced ? [] : previous?.libraries ?? [], entries: replaced ? [] : previous?.entries ?? [],
    observations: replaced ? [] : previous?.observations, diagnostics: replaced ? [] : previous?.diagnostics ?? [],
  } };
}
