import {loadModules} from '../test-module-loader.mjs';
const revision=`sha256:${'a'.repeat(64)}`;
/** Explicit read-model projections for SSR; mounting must never open a live transport. */
export async function installGameReadModelFixture(modules){
  const registered=()=>modules.get('./games')?.getGameCatalogue?.()??[];
  const gameUiState=(state={catalogue:registered()})=>({...state,catalogue:state.catalogue.map(game=>({...game,detailsRevision:revision}))});
  const gameUiSummary=state=>({status:'ready',value:gameUiState(state)});
  const gameUiDetail=(catalogue=registered(),id)=>{const game=catalogue.find(game=>game.id===id);return game?{status:'ready',value:{gameId:id,scope:revision,value:game}}:{status:'connecting'};};
  modules.set('./game-read-models/shared',{
    gameUiState,gameUiSummary,gameUiDetail,
    observeGamesState(listener){listener(gameUiSummary());return()=>{};},
    observeGameDiscovery(listener){listener({status:'connecting'});return()=>{};},
    observeGameDetails(id,listener){listener(gameUiDetail(undefined,id));return()=>{};},
    observeGameJobs(listener){listener({status:'ready',value:{jobs:[]}});return()=>{};},
    observeGameAsset(_reference,listener){listener({status:'error',error:'SSR must not fetch game images.'});return()=>{};},
  });
  modules.set('./game-read-models/image',{
    gameImageSource:reference=>reference&&!reference.startsWith('game-asset:')?reference:undefined,
    gameImage(){throw Error('SSR must not mount image actions.');},
    async resolveGameImage(reference){if(reference.startsWith('game-asset:'))throw Error('SSR must not fetch game images.');return reference;},
  });
  modules.set('./game-read-models/views',(await loadModules(['./game-read-models/views'])).get('./game-read-models/views'));
  return {gameUiState,gameUiSummary,gameUiDetail};
}
/** Effects do not run in SSR. Seed explicit current projections before derived values. */
export function prepareGameComponentSource(source){
  if(!source.includes('let localModel=$state<GameModelState<GamesSummaryState>>'))return source;
  return source.replace('<script lang="ts">',`<script lang="ts">\n  import {gameUiState,gameUiSummary,gameUiDetail} from './game-read-models/shared';`)
    .replace('  let localModel=', '  if(status)status=gameUiState(status);\n  let localModel=')
    .replace("localModel=$state<GameModelState<GamesSummaryState>>({status:'connecting'})",'localModel=$state<GameModelState<GamesSummaryState>>(gameUiSummary(status))')
    .replace("detailModel=$state<GameModelState<GameDetailModel>>({status:'connecting'})",'detailModel=$state<GameModelState<GameDetailModel>>(gameUiDetail(status?.catalogue,selectedGameId))');
}
export function prepareProfileGameSource(source){
  if(!source.includes('let gameState=$state.raw<GameModelState<GamesSummaryState>>'))return source;
  return source.replace('<script lang="ts">',`<script lang="ts">\n  import {gameUiSummary,gameUiDetail} from './game-read-models/shared';`)
    .replace("gameState=$state.raw<GameModelState<GamesSummaryState>>({status:'connecting'})",'gameState=$state.raw<GameModelState<GamesSummaryState>>(gameUiSummary())')
    .replace("  const summaryGame=",'  gameDetailState=gameUiDetail(undefined,selectedGame);\n  const summaryGame=');
}
