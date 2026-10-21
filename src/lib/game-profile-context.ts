import {DEFAULT_PROFILE_ID,type DeepReadonly,type ProfileSummary} from './profiles';
import {isGameId,type GameId} from './games';

export type GameProfileCandidates={gameId:GameId;profiles:ProfileSummary[];preferredProfileId:string|null};
export type GameProfileContextRequest={command:'profile_game_candidates'|'profile_game_preference_remove';gameId:GameId}|{command:'profile_game_preference_save';gameId:GameId;profileId:string};
export type GameProfileContextSend=(request:GameProfileContextRequest)=>Promise<unknown>;
const object=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const exact=(value:unknown,keys:string[]):value is Record<string,unknown>=>object(value)&&Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
const uuid=(value:unknown):value is string=>typeof value==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const game=isGameId;
function summary(value:unknown,gameId:GameId):value is ProfileSummary{
  if(!object(value)||!exact(value,['id','name','revision','isDefault','semanticHash',...(Object.hasOwn(value,'gameIds')?['gameIds']:[])])||!uuid(value.id)||typeof value.name!=='string'||!value.name.length||value.name.trim()!==value.name||[...value.name].length>80||/[\u0000-\u001f\u007f-\u009f]/.test(value.name)||!Number.isSafeInteger(value.revision)||Number(value.revision)<1||value.isDefault!==(value.id===DEFAULT_PROFILE_ID)||typeof value.semanticHash!=='string'||!/^sha256:[a-f0-9]{64}$/.test(value.semanticHash))return false;
  return !Object.hasOwn(value,'gameIds')||Array.isArray(value.gameIds)&&value.gameIds.length<=32&&value.gameIds.every(game)&&new Set(value.gameIds).size===value.gameIds.length&&value.gameIds.includes(gameId);
}
export function captureGameProfileCandidates(value:unknown,gameId:GameId):DeepReadonly<GameProfileCandidates>{
  if(!game(gameId)||!exact(value,['gameId','profiles','preferredProfileId'])||value.gameId!==gameId||!Array.isArray(value.profiles)||value.profiles.length>32||!value.profiles.every(profile=>summary(profile,gameId))||new Set(value.profiles.map(profile=>profile.id)).size!==value.profiles.length||!(value.preferredProfileId===null||uuid(value.preferredProfileId)&&value.profiles.some(profile=>profile.id===value.preferredProfileId)))throw new Error('Matching profiles could not be verified. Refresh this game before choosing a profile.');
  const copy=JSON.parse(JSON.stringify(value)) as GameProfileCandidates;
  for(const profile of copy.profiles){if(profile.gameIds)Object.freeze(profile.gameIds);Object.freeze(profile);}Object.freeze(copy.profiles);return Object.freeze(copy);
}
export class GameProfileContextClient{
  constructor(private send:GameProfileContextSend){}
  private async request(request:GameProfileContextRequest):Promise<DeepReadonly<GameProfileCandidates>>{if(!game(request.gameId))throw new Error('Choose a supported game.');return captureGameProfileCandidates(await this.send(request),request.gameId);}
  candidates(gameId:GameId){return this.request({command:'profile_game_candidates',gameId});}
  async savePreference(gameId:GameId,profileId:string){if(!uuid(profileId))throw new Error('Choose a matching saved profile.');const result=await this.request({command:'profile_game_preference_save',gameId,profileId});if(result.preferredProfileId!==profileId)throw new Error('The remembered profile choice was not confirmed.');return result;}
  async removePreference(gameId:GameId){const result=await this.request({command:'profile_game_preference_remove',gameId});if(result.preferredProfileId!==null)throw new Error('Removing the remembered profile choice was not confirmed.');return result;}
}
