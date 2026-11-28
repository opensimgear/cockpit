import type {HidRecord} from '../controller-hid';

export const MAX_IMPORT_BYTES=64*1024*1024;
export const DISPLAY_REPORT_LIMIT=256;
export const object=(value:unknown):HidRecord=>value&&typeof value==='object'&&!Array.isArray(value)?value as HidRecord:{};
export const rows=(value:unknown):HidRecord[]=>Array.isArray(value)?value.map(object):[];
export const exact=(value:unknown):string=>value===undefined||value===null?'unavailable':typeof value==='object'?JSON.stringify(value):String(value);
export const detailOf=(detail:unknown)=>{const value=object(detail);return Object.keys(object(value.detail)).length?object(value.detail):value;};
export type Field={id:string;collection:string;kind:string;reportId:unknown;usage:unknown;metadata:HidRecord};
export function fieldsOf(detail:unknown):Field[]{
  const source=detailOf(detail),descriptor=object(source.descriptor),result:Field[]=[];
  rows(descriptor.fields).forEach((field,index)=>{
    const globals=object(field.globals);
    result.push({id:`descriptor:${index}`,collection:exact(field.collection),kind:exact(field.kind),reportId:globals.report_id,
      usage:field.primary,metadata:field});
  });
  rows(object(source.capabilities).reports).forEach(report=>{
    for(const [key,prefix] of [['valueCaps','hidp-value'],['buttonCaps','hidp-button']] as const){
      rows(report[key]).forEach((field,index)=>result.push({id:`${prefix}:${index}`,collection:exact(field.linkCollection),kind:exact(report.kind),
        reportId:field.reportId,usage:{page:field.usagePage,usage:field.usage,range:field.range},metadata:field}));
    }
  });
  rows(source.elements).forEach(field=>result.push({id:`iohid-cookie:${field.cookie}`,collection:exact(field.parentCookie),kind:exact(field.reportType),
    reportId:field.reportId,usage:{page:field.usagePage,id:field.usage},metadata:field}));
  return result;
}
export type TreeNode={path:string;name:string;value:string;depth:number};
/** Expand only a bounded visible page; the exact native contract remains available below it. */
export function descriptorTree(detail:unknown,search=''):TreeNode[]{
  const result:TreeNode[]=[],query=search.trim().toLowerCase();let visited=0;
  const walk=(value:unknown,path:string,depth:number)=>{
    if(depth>64||visited++>=50000||result.length>=512)return;
    const container=value!==null&&typeof value==='object';
    const line=container?(Array.isArray(value)?`${value.length} entries`:`${Object.keys(value as object).length} properties`):exact(value);
    if(!query||`${path} ${line}`.toLowerCase().includes(query))result.push({path,name:path.split('.').at(-1)??path,value:line,depth});
    if(container)for(const [key,child] of Object.entries(value))walk(child,`${path}.${key}`,depth+1);
  };
  walk(detailOf(detail),'interface',0);return result;
}
export function sampleOf(value:HidRecord):HidRecord{
  const sample=object(value.sample);
  if('state' in sample)return sample;
  for(const [key,state] of [['Integer','valid'],['Null','null'],['Invalid','invalid']] as const){
    if(key in sample)return {state,kind:'integer',value:sample[key]};
  }
  if('Array' in sample)return {state:'valid',kind:'array-selector',...object(sample.Array)};
  if('Bytes' in sample)return {state:'valid',kind:'bytes',bytes:sample.Bytes};
  return {state:'unknown',...object(sample.Unknown)};
}
export function valuePresentation(value:HidRecord,field?:Field){
  const sample=sampleOf(value),metadata=field?.metadata??{},globals=object(metadata.globals);
  const logical=object(value.logical),min=logical.minimum??globals.logical_min??metadata.logicalMin,max=logical.maximum??globals.logical_max??metadata.logicalMax;
  const semantics=exact(value.semantics).toLowerCase();
  const usage=JSON.stringify(value.usages??field?.usage??value.usage??'');
  const hat=/"(?:id|usage)":57\b/.test(usage)||(value.usagePage===1&&value.usage===57);
  const button=field?.id.startsWith('hidp-button')||value.usagePage===9||/"page":9\b/.test(usage)||/"usagePage":9\b/.test(usage);
  const bitfield=(metadata.bitSize===1||globals.size===1)&&!button;
  const kind=sample.kind==='array-selector'?'Usage selectors':sample.kind==='bytes'?'Opaque bytes':semantics==='relative'?'Relative delta':semantics==='event'?'Event':hat?'Hat':button?'Button':bitfield?'Bitfield':semantics==='absolute'?'Absolute position':'Opaque value';
  const raw=sample.value??sample.selector??sample.bytes??sample.encoded;
  let percent:number|null=null;
  // BigInt keeps wide values exact. Only the final bounded visual ratio becomes a Number.
  if(sample.state==='valid'&&kind==='Absolute position'&&/^-?\d+$/.test(exact(raw))&&/^-?\d+$/.test(exact(min))&&/^-?\d+$/.test(exact(max))){
    const lo=BigInt(String(min)),hi=BigInt(String(max)),n=BigInt(String(raw));
    if(hi>lo&&n>=lo&&n<=hi)percent=Number((n-lo)*10000n/(hi-lo))/100;
  }
  return {kind,quality:exact(sample.state),text:exact(raw),min:exact(min),max:exact(max),percent,
    active:sample.state==='valid'&&button&&raw!==undefined&&String(raw)!=='0',sample};
}
export function reportValues(reports:HidRecord[]):HidRecord[]{
  return reports.flatMap(report=>{
    const projection=object(report.samples??report.decoded),values:HidRecord[]=[];
    values.push(...rows(projection.values).map(value=>({...value,fieldId:value.fieldId??`descriptor:${value.field}`,elementIndex:value.elementIndex??value.element})));
    for(const data of rows(projection.data))for(const value of rows(data.values))values.push({fieldId:`hidp-value:${value.capabilityIndex}:data:${data.dataIndex}`,
      elementIndex:0,semantics:value.semantics,sample:{state:value.quality,kind:value.semantics==='array-selector'?'array-selector':'integer',value:value.native},encoded:data.rawValue});
    for(const value of rows(projection.elements))values.push({fieldId:`iohid-cookie:${value.cookie}`,elementIndex:0,
      semantics:value.relative===true?'relative':value.relative===false?'absolute':'opaque',usagePage:value.usagePage,usage:value.usage,
      sample:typeof value.integer==='string'?{state:'valid',kind:'integer',value:value.integer}:{state:'unsupported',reason:'No exact native scalar'},elementValueHex:value.valueHex});
    return values.map(value=>({...value,reportId:report.reportId??report.windowsReportId,sequence:report.sequence,receivedUs:report.receivedUs}));
  });
}
export type Filters={kind:string;report:string;collection:string;usage:string;changed:boolean;active:boolean};
export const emptyFilters=():Filters=>({kind:'',report:'',collection:'',usage:'',changed:false,active:false});
export function matches(field:Field|undefined,value:HidRecord,filters:Filters,changed:Set<string>):boolean{
  if(filters.kind&&field?.kind!==filters.kind)return false;
  if(filters.report&&exact(value.reportId??field?.reportId)!==filters.report)return false;
  if(filters.collection&&!exact(field?.collection).toLowerCase().includes(filters.collection.toLowerCase()))return false;
  if(filters.usage&&!JSON.stringify([value.usages,value.usage,field?.usage,value.fieldId]).toLowerCase().includes(filters.usage.toLowerCase()))return false;
  if(filters.changed&&!changed.has(`${value.fieldId}:${value.elementIndex}`))return false;
  return !filters.active||valuePresentation(value,field).active;
}
export function appendReports(previous:HidRecord[],next:HidRecord[]){
  const key=(row:HidRecord)=>`${exact(row.kind??'report')}:${exact(row.sequence??row.fromSequence)}:${exact(row.receivedUs)}`;
  const merged=new Map(previous.map(row=>[key(row),row]));
  for(const row of next)merged.set(key(row),row);
  return [...merged.values()].slice(-DISPLAY_REPORT_LIMIT);
}
