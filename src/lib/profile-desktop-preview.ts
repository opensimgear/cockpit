/** Synthetic recording uses a captured compiled program and cannot submit input. */
import{profileScalarAccepts,type DeepReadonly,type ProfileEndpoint,type ProfileScalarSchema,type ProfileValue}from'./profiles.ts';
import type{CompiledSource,CompilationSummary,ProfileCompilerClient}from'./profile-compilation';
import{desktopKeyLabel}from'./profile-desktop.ts';
export type DesktopPreviewFrame={source:string;generation:string;sequence:string;capturedUs:string;reason:'baseline'|'change';changes:{portId:string;sample:{status:'known';value:{kind:'scalar';value:Exclude<ProfileValue,null>}}}[]};
export type DesktopPreviewOperation={sequence:string;operationId:string;actionId:string;atUs:string;events:unknown[];collision:boolean;held:unknown[]};
export type DesktopPreviewResult={compilationId:string;profileId:string;profileRevision:number;programHash:string;mode:'recording';nativeSubmission:false;operations:DesktopPreviewOperation[];decisions:{atUs:string;actionId:string|null;kind:string;detail:string}[];trace:unknown;fault:string|null;droppedDecisions:string;limitations:string};
export type DesktopPreviewRequest={command:'profile_desktop_preview';compilationId:string;frames:DesktopPreviewFrame[];advances:string[]};
const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
const counter=(value:unknown):value is string=>typeof value==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(value)&&BigInt(value)<=18_446_744_073_709_551_615n;
export function desktopPreviewEventLabel(value:unknown):string{
 if(!object(value))return 'Unknown recorded event';
 if(value.kind==='key'&&typeof value.usage==='number')return`${value.repeat?'Repeat':value.down?'Press':'Release'} ${desktopKeyLabel(value.usage)}`;
 if(value.kind==='mouse-button')return`${value.down?'Press':'Release'} ${String(value.button)} mouse button`;
 if(value.kind==='pointer')return`Move pointer ${String(value.dx)}, ${String(value.dy)}`;
 if(value.kind==='wheel')return`Scroll horizontal ${String(value.horizontal)}, vertical ${String(value.vertical)}`;
 return JSON.stringify(value);
}
export function desktopPreviewHeldLabel(values:unknown[]):string{return values.map(value=>object(value)&&value.kind==='key'&&typeof value.key==='number'?desktopKeyLabel(value.key):object(value)&&value.kind==='mouse-button'?`${String(value.button)} mouse button`:JSON.stringify(value)).join(', ')||'None';}
export function previewSourceKey(endpoint:DeepReadonly<ProfileEndpoint>):string{return JSON.stringify([endpoint.nodeId,endpoint.portId]);}
export function previewScalar(source:DeepReadonly<CompiledSource>):DeepReadonly<ProfileScalarSchema>|null{return source.contract.delivery==='state'||source.contract.delivery==='event'?source.contract.schema:null;}
export function previewNeutral(source:DeepReadonly<CompiledSource>):Exclude<ProfileValue,null>{
 const contract=source.contract,schema=previewScalar(source);if(!schema)throw Error('Recording accepts exact scalar source ports only.');
 if(contract.delivery==='state'&&contract.fallback!==null)return contract.fallback;
 if(schema.type==='boolean')return false;if(schema.type==='text')return'';if(schema.type==='enum')return schema.values[0];if(schema.type==='rgb')return'000000';return Math.min(schema.max,Math.max(schema.min,0));
}
export function previewPressed(source:DeepReadonly<CompiledSource>):Exclude<ProfileValue,null>{const schema=previewScalar(source);if(!schema)throw Error('Choose a scalar source.');if(schema.type==='boolean')return true;if(schema.type==='text')return'Preview'.slice(0,schema.maxBytes);if(schema.type==='enum')return schema.values[0];if(schema.type==='rgb')return'ffffff';return schema.max;}
export function desktopPreviewFrames(sources:readonly DeepReadonly<CompiledSource>[],selected:string,value:Exclude<ProfileValue,null>):{frames:DesktopPreviewFrame[];advances:string[]}{
 if(sources.length>1024||new Set(sources.map(source=>previewSourceKey(source.endpoint))).size!==sources.length)throw Error('Recording needs unique bounded captured sources.');
 const groups=new Map<string,DesktopPreviewFrame['changes']>();
 for(const source of sources){if(!previewScalar(source))throw Error('Recording accepts scalar sources only.');const changes=groups.get(source.endpoint.nodeId)??[];changes.push({portId:source.endpoint.portId,sample:{status:'known',value:{kind:'scalar',value:previewNeutral(source)}}});groups.set(source.endpoint.nodeId,changes);}
 const frames:DesktopPreviewFrame[]=[...groups].sort(([a],[b])=>a.localeCompare(b)).map(([source,changes])=>({source,generation:'1',sequence:'1',capturedUs:'0',reason:'baseline',changes:changes.sort((a,b)=>a.portId.localeCompare(b.portId))}));
 if(sources.length){const source=sources.find(source=>previewSourceKey(source.endpoint)===selected),schema=source&&previewScalar(source);if(!source||!schema||!profileScalarAccepts(schema as ProfileScalarSchema,value))throw Error('Choose a captured scalar source and a value inside its exact contract.');
  for(const[sequence,at,signal]of[['2','1000',value],['3','1000000',previewNeutral(source)]]as const)frames.push({source:source.endpoint.nodeId,generation:'1',sequence,capturedUs:at,reason:'change',changes:[{portId:source.endpoint.portId,sample:{status:'known',value:{kind:'scalar',value:signal}}}]});
 }
 if(frames.length>256||frames.reduce((sum,frame)=>sum+frame.changes.length,0)>4096)throw Error('This capture exceeds the recording frame or change budget.');
 return{frames,advances:Array.from({length:150},(_,index)=>String((index+1)*8000))};
}
export async function desktopPreviewSources(compiler:Pick<ProfileCompilerClient,'inspect'>,preview:DeepReadonly<CompilationSummary>):Promise<CompiledSource[]>{
 const sources:CompiledSource[]=[];let total=preview.sectionTotals.sources;
 if(total>1024)throw Error('This capture exceeds the source-port recording budget.');
 do{const page=await compiler.inspect(preview,'sources',sources.length,64);if(page.total!==total||page.items.length===0&&sources.length<total)throw Error('The captured source catalogue changed or made no progress. Refresh preflight.');sources.push(...JSON.parse(JSON.stringify(page.items))as CompiledSource[]);}while(sources.length<total);
 return sources;
}
export function desktopPreviewResult(value:unknown,preview:DeepReadonly<CompilationSummary>):DesktopPreviewResult{
 if(!object(value)||value.compilationId!==preview.compilationId||value.profileId!==preview.profileId||value.profileRevision!==preview.profileRevision||value.programHash!==preview.programHash||value.mode!=='recording'||value.nativeSubmission!==false||!Array.isArray(value.operations)||value.operations.length>4096||!value.operations.every(operation=>object(operation)&&counter(operation.sequence)&&counter(operation.operationId)&&counter(operation.atUs)&&typeof operation.actionId==='string'&&Array.isArray(operation.events)&&operation.events.length<=128&&Array.isArray(operation.held)&&operation.held.length<=64&&typeof operation.collision==='boolean')||!Array.isArray(value.decisions)||value.decisions.length>4096||!value.decisions.every(decision=>object(decision)&&counter(decision.atUs)&&(decision.actionId===null||typeof decision.actionId==='string')&&typeof decision.kind==='string'&&typeof decision.detail==='string')||!(value.fault===null||typeof value.fault==='string')||!counter(value.droppedDecisions)||typeof value.limitations!=='string')throw Error('The recording does not match this exact captured program or its no-submission contract.');
 if(new TextEncoder().encode(JSON.stringify(value)).length>1_900_000)throw Error('The recording exceeds its reply budget.');
 return JSON.parse(JSON.stringify(value))as DesktopPreviewResult;
}
