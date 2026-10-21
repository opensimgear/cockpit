import {isBuiltinGameId as isGameId,type GameId} from './games';
import type {DeepReadonly} from './profiles';

export type GameTelemetryScalar=string|number|boolean|null;
export type GameTelemetryChannel={id:string;label:string;category:string;type:string;unit:string|null;status:string;value:GameTelemetryScalar|GameTelemetryScalar[];ageMs:number|null;nativeId?:string|null;nativeField?:string|null;provider?:string|null;group?:string|null;description?:string|null};
export type GameTelemetryGroup={id:string;label:string;total:number};
export type GameTelemetryGame={id:GameId;name:string;status:string;source:string;message?:string|null;packet?:number|string|null;ageMs?:number|null;context:Record<string,GameTelemetryScalar>|null;groups:GameTelemetryGroup[];total:number;offset:number;limit:number;channels:GameTelemetryChannel[]};
export type GameTelemetryReport={schemaVersion:1;games:GameTelemetryGame[];capturedAt?:string};
export type GameTelemetryRequest={command:'game_telemetry_inspect';gameId?:GameId;offset:number;limit:number;search?:string;group?:string};
export type GameTelemetryQuery=Omit<GameTelemetryRequest,'command'>;
export type GameTelemetryState={query:GameTelemetryQuery;report:DeepReadonly<GameTelemetryReport>|null;active:boolean;loading:boolean;stale:boolean;error:string;updatedAt:number|null};

const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
const text=(value:unknown,max=256):value is string=>typeof value==='string'&&value.length>0&&value.length<=max&&!/[\u0000-\u001f\u007f-\u009f]/.test(value);
const whole=(value:unknown,max=65536):value is number=>Number.isSafeInteger(value)&&Number(value)>=0&&Number(value)<=max;
const age=(value:unknown)=>value===null||typeof value==='number'&&Number.isFinite(value)&&value>=0;
const scalar=(value:unknown):value is GameTelemetryScalar=>value===null||typeof value==='boolean'||typeof value==='number'&&Number.isFinite(value)||typeof value==='string'&&value.length<=4096;
function channel(value:unknown):value is GameTelemetryChannel{
  return object(value)&&text(value.id)&&text(value.label)&&text(value.category)&&text(value.type,80)&&text(value.status,80)
    &&(value.unit===null||typeof value.unit==='string'&&value.unit.length<=80)&&age(value.ageMs)
    &&(scalar(value.value)||Array.isArray(value.value)&&value.value.length<=64&&value.value.every(scalar))
    &&['nativeId','nativeField','provider','group','description'].every(key=>value[key]===undefined||value[key]===null||typeof value[key]==='string'&&String(value[key]).length<=4096);
}
function game(value:unknown):value is GameTelemetryGame{
  if(!object(value)||!isGameId(value.id)||!text(value.name)||!text(value.status,80)||typeof value.source!=='string'||value.source.length>4096
    ||!Array.isArray(value.groups)||value.groups.length>256||!value.groups.every(group=>object(group)&&text(group.id)&&text(group.label)&&whole(group.total))
    ||!whole(value.total)||!whole(value.offset)||!whole(value.limit,128)||value.limit===0||!Array.isArray(value.channels)||value.channels.length>128||!value.channels.every(channel)
    ||value.channels.length>value.limit||value.channels.length>0&&value.offset+value.channels.length>value.total||new Set(value.channels.map(channel=>channel.id)).size!==value.channels.length
    ||new Set(value.groups.map(group=>group.id)).size!==value.groups.length)return false;
  return(value.context===null||object(value.context)&&Object.keys(value.context).length<=64&&Object.entries(value.context).every(([key,value])=>text(key)&&scalar(value)))
    &&(value.packet===undefined||value.packet===null||typeof value.packet==='string'&&value.packet.length<=256||typeof value.packet==='number'&&Number.isFinite(value.packet))
    &&(value.ageMs===undefined||age(value.ageMs))&&(value.message===undefined||value.message===null||typeof value.message==='string'&&value.message.length<=4096);
}
export function isGameTelemetryReport(result:unknown):result is GameTelemetryReport{return object(result)&&result.schemaVersion===1&&Array.isArray(result.games)&&result.games.length>0&&result.games.length<=7&&result.games.every(game)&&new Set(result.games.map(game=>game.id)).size===result.games.length&&JSON.stringify(result).length<=1024*1024&&(result.capturedAt===undefined||text(result.capturedAt,256));}
function freeze<T>(value:T):DeepReadonly<T>{
  const copy=JSON.parse(JSON.stringify(value));
  const visit=(entry:unknown)=>{if(entry&&typeof entry==='object'){Object.values(entry).forEach(visit);Object.freeze(entry);}};visit(copy);return copy;
}
export class GameTelemetryClient{
  constructor(private send:(request:GameTelemetryRequest)=>Promise<unknown>){}
  async inspect(query:GameTelemetryQuery):Promise<DeepReadonly<GameTelemetryReport>>{
    const result=await this.send({command:'game_telemetry_inspect',...query});
    if(!isGameTelemetryReport(result)
      ||query.gameId&&!result.games.some(game=>game.id===query.gameId))throw Error('The native game telemetry reply is invalid or incomplete.');
    return freeze(result as unknown as GameTelemetryReport);
  }
}
const initial=():GameTelemetryState=>({query:{offset:0,limit:64},report:null,active:false,loading:false,stale:true,error:'',updatedAt:null});
/** Passive captures only. A single outstanding request and query epochs keep
 * old game/filter pages out of the selected view, including hide/resume races. */
export class GameTelemetryController{
  private value=initial();private epoch=0;private inFlight=false;private queued=false;private disposed=false;
  private listeners=new Set<(state:DeepReadonly<GameTelemetryState>)=>void>();
  constructor(private client:Pick<GameTelemetryClient,'inspect'>,private now=()=>Date.now()){}
  get state():DeepReadonly<GameTelemetryState>{return Object.freeze(this.value);}
  subscribe(listener:(state:DeepReadonly<GameTelemetryState>)=>void):()=>void{this.listeners.add(listener);listener(this.state);return()=>this.listeners.delete(listener);}
  private set(patch:Partial<GameTelemetryState>){if(this.disposed)return;this.value={...this.value,...patch};for(const listener of this.listeners)listener(this.state);}
  configure(query:GameTelemetryQuery):void{
    if(this.disposed)return;
    if(query.gameId!==undefined&&!isGameId(query.gameId)||!whole(query.offset)||!whole(query.limit,128)||!query.limit
      ||query.search!==undefined&&query.search.length>200||query.group!==undefined&&query.group.length>256)throw Error('Choose a bounded game telemetry page.');
    const next={...query,search:query.search?.trim()||undefined,group:query.group||undefined};
    if(JSON.stringify(next)===JSON.stringify(this.value.query))return;
    this.epoch++;this.queued=this.inFlight;this.set({query:freeze(next),report:null,loading:false,stale:true,error:'',updatedAt:null});
  }
  setActive(active:boolean):void{
    if(this.disposed||active===this.value.active)return;
    this.epoch++;this.queued=active&&this.inFlight;this.set({active,loading:false,stale:true});
  }
  async refresh():Promise<boolean>{
    if(this.disposed||!this.value.active||this.inFlight)return false;
    const epoch=this.epoch,query={...this.value.query};this.inFlight=true;this.set({loading:true,error:''});
    try{
      const report=await this.client.inspect(query);if(this.disposed||epoch!==this.epoch||!this.value.active)return false;
      this.set({report,stale:false,updatedAt:this.now()});return true;
    }catch(error){if(!this.disposed&&epoch===this.epoch)this.set({stale:true,error:error instanceof Error?error.message:String(error)});return false;}
    finally{
      this.inFlight=false;if(epoch===this.epoch)this.set({loading:false});
      if(this.queued){this.queued=false;if(!this.disposed&&this.value.active)void this.refresh();}
    }
  }
  dispose():void{this.epoch++;this.queued=false;this.set({...initial()});this.disposed=true;this.listeners.clear();}
}
const statuses:Record<string,string>={live:'Live',ready:'Live',receiving:'Live',paused:'Paused',stale:'Stale',waiting:'Waiting',unknown:'Unknown',known:'Known',unavailable:'Unavailable',unsupported:'Unsupported',error:'Error','not-running':'Game closed','not-installed':'Not installed','setup-required':'Setup required'};
export const telemetryStatus=(value:string)=>statuses[value]??value.replace(/[-_]/g,' ').replace(/^./,letter=>letter.toUpperCase());
export function telemetryAge(value:number|null|undefined):string{return value===null||value===undefined?'Not observed':value<1000?`${Math.round(value)} ms`:`${(value/1000).toFixed(value<10000?1:0)} s`;}
function scalarText(value:GameTelemetryScalar):string{return value===null?'—':typeof value==='boolean'?value?'True':'False':typeof value==='number'?new Intl.NumberFormat('en-US',{maximumSignificantDigits:10,useGrouping:false}).format(value):value||'Empty';}
export function telemetryValue(channel:DeepReadonly<GameTelemetryChannel>,stale=false):string{
  if(stale||channel.status!=='known'||channel.value===null)return'—';
  return Array.isArray(channel.value)?`[${channel.value.map(scalarText).join(', ')}]`:scalarText(channel.value as GameTelemetryScalar);
}
export function telemetryContext(context:DeepReadonly<GameTelemetryGame['context']>):{label:string;value:string}[]{return context?Object.entries(context).map(([key,value])=>({label:key.replace(/([a-z])([A-Z])/g,'$1 $2').replace(/[_-]/g,' '),value:scalarText(value)})):[];}
export function preferredTelemetryGame(games:readonly DeepReadonly<GameTelemetryGame>[]):GameId|undefined{return(games.find(game=>['live','ready','receiving','paused'].includes(game.status))??games.find(game=>game.status!=='not-installed')??games[0])?.id;}
