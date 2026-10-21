/** Native game rules and explicit selection mode. Reading this client never selects or applies a profile. */
import {captureProfileSummaries,type DeepReadonly,type ProfileSelectionMode,type ProfileSummary} from './profiles';
import {supportedGames,type GameId} from './games';
export type AutomaticGameId=GameId;
export type ProfileGameRule={identity:{gameId:AutomaticGameId;executable:string};profileId:string;priority:number;enabled:boolean};
export type ProfileAutomaticTarget={profileId:string;gameId:AutomaticGameId|null;processSession:string|null};
export type ProfileAutomaticStatus={schemaVersion:1;mode:ProfileSelectionMode;manualProfileId:string;rules:ProfileGameRule[];status:'manual'|'waiting'|'blocked'|'conflict'|'preparing'|'active'|'failed';message:string;conflicts:AutomaticGameId[];lastAttempt:ProfileAutomaticTarget|null;target:ProfileAutomaticTarget|null;readOnly:boolean};
export type ProfileAutomaticCommand='profile_automatic_status'|'profile_game_rule_save'|'profile_game_rule_remove';
export type ProfileAutomaticRequest={command:'profile_automatic_status'|'profile_list'}|{command:'profile_selection_mode';mode:ProfileSelectionMode}|{command:'profile_game_rule_save';gameId:AutomaticGameId;profileId:string;priority:number;enabled:boolean}|{command:'profile_game_rule_remove';gameId:AutomaticGameId};
export type ProfileAutomaticSend=(request:ProfileAutomaticRequest)=>Promise<unknown>;
const games:readonly string[]=supportedGames.map(game=>game.id);
const object=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const exact=(value:unknown,keys:string[]):value is Record<string,unknown>=>object(value)&&Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
const uuid=(value:unknown):value is string=>typeof value==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const text=(value:unknown,max:number,empty=false):value is string=>typeof value==='string'&&(empty||value.length>0)&&new TextEncoder().encode(value).length<=max&&!/[\u0000-\u001f\u007f-\u009f]/.test(value);
const game=(value:unknown):value is AutomaticGameId=>typeof value==='string'&&games.includes(value);
const priority=(value:unknown):value is number=>typeof value==='number'&&Number.isInteger(value)&&value>=-32768&&value<=32767;
const mode=(value:unknown):value is ProfileSelectionMode=>value==='manual'||value==='automatic';
function frozen<T>(value:T):DeepReadonly<T>{const copy:T=JSON.parse(JSON.stringify(value));const freeze=(entry:unknown)=>{if(entry&&typeof entry==='object'){Object.values(entry).forEach(freeze);Object.freeze(entry);}};freeze(copy);return copy as DeepReadonly<T>;}
function identity(value:unknown):boolean{return exact(value,['gameId','executable'])&&game(value.gameId)&&text(value.executable,32768)&&(/^[^:]:[\\/]/.test(value.executable)||value.executable.startsWith('\\\\'));}
function rule(value:unknown):value is ProfileGameRule{return exact(value,['identity','profileId','priority','enabled'])&&identity(value.identity)&&uuid(value.profileId)&&priority(value.priority)&&typeof value.enabled==='boolean';}
function target(value:unknown):boolean{return value===null||exact(value,['profileId','gameId','processSession'])&&uuid(value.profileId)&&(value.gameId===null?value.processSession===null:game(value.gameId)&&text(value.processSession,256));}
export function captureAutomaticStatus(value:unknown):DeepReadonly<ProfileAutomaticStatus>{
  if(!exact(value,['schemaVersion','mode','manualProfileId','rules','status','message','conflicts','lastAttempt','target','readOnly'])||value.schemaVersion!==1||!mode(value.mode)||!uuid(value.manualProfileId)||!Array.isArray(value.rules)||value.rules.length>games.length||!value.rules.every(rule)||new Set(value.rules.map(rule=>rule.identity.gameId)).size!==value.rules.length||typeof value.status!=='string'||!['manual','waiting','blocked','conflict','preparing','active','failed'].includes(value.status)||!text(value.message,16384,true)||!Array.isArray(value.conflicts)||value.conflicts.length>games.length||!value.conflicts.every(game)||new Set(value.conflicts).size!==value.conflicts.length||!target(value.lastAttempt)||!target(value.target)||typeof value.readOnly!=='boolean')throw new Error('Native automatic profile status is malformed. Refresh the native service before changing rules.');
  return frozen(value as unknown as ProfileAutomaticStatus);
}
export function captureAutomaticProfiles(value:unknown):DeepReadonly<ProfileSummary[]>{
  try{return captureProfileSummaries(value);}catch{throw new Error('The unified saved-profile list is malformed. Refresh Profiles before choosing a rule.');}
}
/** Browser requests contain a supported game ID, never an executable or process capability. */
export class ProfileAutomaticClient{
  constructor(private send:ProfileAutomaticSend){}
  private async request(request:ProfileAutomaticRequest):Promise<DeepReadonly<ProfileAutomaticStatus>>{const result=await this.send(request);return captureAutomaticStatus(object(result)?result.automatic:undefined);}
  status():Promise<DeepReadonly<ProfileAutomaticStatus>>{return this.request({command:'profile_automatic_status'});}
  async profiles():Promise<DeepReadonly<ProfileSummary[]>>{const result=await this.send({command:'profile_list'});return captureAutomaticProfiles(object(result)?result.profiles:undefined);}
  async selectionMode(value:ProfileSelectionMode):Promise<DeepReadonly<ProfileAutomaticStatus>>{if(!mode(value))throw new Error('Choose Manual or Automatic explicitly.');const status=await this.request({command:'profile_selection_mode',mode:value});if(status.mode!==value)throw new Error('Native selection mode did not confirm the explicit choice.');return status;}
  async saveRule(gameId:AutomaticGameId,profileId:string,weight:number,enabled:boolean):Promise<DeepReadonly<ProfileAutomaticStatus>>{
    if(!game(gameId)||!uuid(profileId)||!priority(weight)||typeof enabled!=='boolean')throw new Error('Choose a supported game, saved profile and integer priority from -32768 to 32767.');
    const status=await this.request({command:'profile_game_rule_save',gameId,profileId,priority:weight,enabled});const saved=status.rules.find(rule=>rule.identity.gameId===gameId);if(!saved||saved.profileId!==profileId||saved.priority!==weight||saved.enabled!==enabled)throw new Error('The native rule reply does not confirm the exact saved rule.');return status;
  }
  async removeRule(gameId:AutomaticGameId):Promise<DeepReadonly<ProfileAutomaticStatus>>{if(!game(gameId))throw new Error('Choose a supported game rule.');const status=await this.request({command:'profile_game_rule_remove',gameId});if(status.rules.some(rule=>rule.identity.gameId===gameId))throw new Error('Native did not confirm this rule was removed.');return status;}
}
