import { profileDocumentError, type DeepReadonly, type ProfileDocument, type ProfileOperation, type ProfilePortContract, type ProfileProcessingNode, type ProfileScalarSchema } from './profiles';
import { graphConnectionError, graphDraft, graphFreshId, graphSetProcessing, type ProfileGraphDraft } from './profile-graph';
import { processingNodeError, processingTemplate, processingUnitConversion, type ProcessingPreset } from './profile-processing';

export type ProcessingParameters = Record<string, unknown>;
type Numeric = Extract<ProfileScalarSchema,{type:'number'|'integer'}>;
const clone=<T>(value:DeepReadonly<T>):T=>JSON.parse(JSON.stringify(value));
const numeric=(schema:DeepReadonly<ProfileScalarSchema>|undefined):schema is DeepReadonly<Numeric>=>schema?.type==='number'||schema?.type==='integer';
const scalar=(contract:DeepReadonly<ProfilePortContract>|undefined)=>contract&&'schema' in contract?contract.schema:undefined;
const state=(schema:ProfileScalarSchema):ProfilePortContract=>({delivery:'state',schema,freshnessMs:1000,fallback:null});
const defaultContract=(preset:ProcessingPreset):ProfilePortContract=>{
  const boolean=['not','any','all','select','toggle','pulse','tap','hold','double-tap','macro'].includes(preset);
  const schema:ProfileScalarSchema=boolean?{type:'boolean'}:{type:'number',min:0,max:1,unit:preset==='unit-convert'?'m':preset==='ratio-to-percent'?'ratio':null};
  return preset==='event-to-state'?{delivery:'event',schema}:state(schema);
};
const equal=(a:unknown,b:unknown)=>JSON.stringify(a)===JSON.stringify(b);

/** Recalculate the declared numeric output when parameters change. No edge
 * conversion or downstream widening happens here: graphSetProcessing checks
 * every existing connection against this new, exact output contract. */
export function processingDerivedNode(value:DeepReadonly<ProfileProcessingNode>):ProfileProcessingNode {
  const node=clone<ProfileProcessingNode>(value),op=node.operation,input=scalar(node.inputs.find(port=>port.id==='value')?.contract??node.inputs[0]?.contract),output=node.outputs[0]?.contract;
  if(!numeric(input)||!output||!('schema' in output))return node;
  const setNumeric=(min:number,max:number,unit:string|null=input.unit,type:'number'|'integer'=input.type)=>{
    if(!Number.isFinite(min)||!Number.isFinite(max))throw new Error('The transformation produces a nonfinite output range.');
    output.schema={type,min:Math.min(min,max),max:Math.max(min,max),unit};
    if(output.delivery==='state')output.fallback=null;
  };
  switch(op.kind){
    case 'scale':setNumeric(input.min*op.factor+op.offset,input.max*op.factor+op.offset,input.unit,input.type==='integer'&&Number.isInteger(op.factor)&&Number.isInteger(op.offset)?'integer':'number');break;
    case 'clamp':if(op.min>op.max)throw new Error('Clamp minimum must not exceed its maximum.');setNumeric(Math.min(op.max,Math.max(op.min,input.min)),Math.min(op.max,Math.max(op.min,input.max)));break;
    case 'range-map':{
      if(op.inputMin>=op.inputMax)throw new Error('Map range requires different input minimum and maximum.');
      const factor=(op.outputMax-op.outputMin)/(op.inputMax-op.inputMin),offset=op.outputMin-op.inputMin*factor;
      const round=(value:number)=>op.rounding==='nearest'?(value<0?-Math.round(-value):Math.round(value)):op.rounding==='floor'?Math.floor(value):op.rounding==='ceil'?Math.ceil(value):op.rounding==='truncate'?Math.trunc(value):value;
      const type=numeric(output.schema)?output.schema.type:input.type;setNumeric(round(input.min*factor+offset),round(input.max*factor+offset),input.unit,type);break;
    }
    case 'unit-convert':{
      const conversion=processingUnitConversion(op.from,op.to);if(!conversion)throw new Error('Choose a supported explicit unit conversion.');
      if(op.clamp)break;
      const [factor,offset]=conversion,round=(value:number)=>op.rounding==='nearest'?(value<0?-Math.round(-value):Math.round(value)):op.rounding==='floor'?Math.floor(value):op.rounding==='ceil'?Math.ceil(value):op.rounding==='truncate'?Math.trunc(value):value;
      setNumeric(round(input.min*factor+offset),round(input.max*factor+offset),op.to,numeric(output.schema)?output.schema.type:'number');break;
    }
    case 'curve':if(op.points.length)setNumeric(Math.min(...op.points.map(point=>point.output)),Math.max(...op.points.map(point=>point.output)),input.unit,numeric(output.schema)?output.schema.type:input.type);break;
    case 'declare-quantity':setNumeric(input.min,input.max,op.unit);break;
  }
  return node;
}

/** A dropped node is persisted with its chosen operation, but remains blocked
 * from activation until an explicit source establishes the actual contract. */
export function createProcessingDraft(preset:ProcessingPreset,id:string,parameters:ProcessingParameters={}):ProfileProcessingNode {
  let node=processingTemplate(preset,id,defaultContract(preset));
  const keys=Object.keys(parameters);for(const key of keys)if(key==='kind'||!Object.hasOwn(node.operation,key))throw new Error('Configure only parameters belonging to the selected transformation.');
  node.operation={...node.operation,...clone(parameters)} as ProfileOperation;
  node=processingDerivedNode(node);
  if(preset!=='constant')node.draft={preset,...(keys.length?{configured:keys}:{} )};
  const error=processingNodeError(node);if(error)throw new Error(error);return node;
}

export function isProcessingDraft(document:DeepReadonly<ProfileDocument>,nodeId:string):boolean { return !!document.processingNodes.find(node=>node.id===nodeId)?.draft; }

export function graphDropProcessing(value:DeepReadonly<ProfileGraphDraft>,preset:ProcessingPreset,parameters:ProcessingParameters={},activeVehicleTabId=''):ProfileGraphDraft {
  return graphSetProcessing(value,createProcessingDraft(preset,graphFreshId(value.document,'transform'),parameters),activeVehicleTabId);
}

/** Resolve only the pending data input. A known gate control must not guess
 * the type of its separate value input. No existing resolved block is retyped. */
export function resolveProcessingNode(previous:DeepReadonly<ProfileProcessingNode>,sourceContract:DeepReadonly<ProfilePortContract>,inputPortId?:string):ProfileProcessingNode {
  if(!previous?.draft)throw new Error('Only a transformation awaiting its first input can establish a new signal contract.');
  const nodeId=previous.id,preset=previous.draft.preset as ProcessingPreset;
  if(preset==='gate'&&inputPortId==='gate')throw new Error('Connect the value input first to establish the gated signal type.');
  const baseline=createProcessingDraft(preset,nodeId),node=processingTemplate(preset,nodeId,sourceContract);
  const explicit=new Set(previous.draft.configured??[]),old=previous.operation as unknown as Record<string,unknown>,defaults=baseline.operation as unknown as Record<string,unknown>;
  const parameters=Object.fromEntries(Object.entries(old).filter(([key,item])=>key!=='kind'&&(explicit.has(key)||!equal(item,defaults[key]))));
  node.operation={...node.operation,...clone(parameters)} as ProfileOperation;node.name=previous.name;
  const resolved=processingDerivedNode(node),error=processingNodeError(resolved);if(error)throw new Error(error);
  return resolved;
}

export function resolveProcessingDraft(value:DeepReadonly<ProfileGraphDraft>,nodeId:string,sourceContract:DeepReadonly<ProfilePortContract>,inputPortId?:string):ProfileGraphDraft {
  const next=graphDraft(value),previous=next.document.processingNodes.find(node=>node.id===nodeId);
  if(!previous)throw new Error('Choose an existing unresolved transformation.');
  return graphSetProcessing(next,resolveProcessingNode(previous,sourceContract,inputPortId));
}

/** Replace a pending preset/configuration without losing its canvas identity,
 * tab, position or history. A connected block uses explicit contract edits. */
export function updateProcessingDraft(value:DeepReadonly<ProfileGraphDraft>,nodeId:string,preset:ProcessingPreset,parameters:ProcessingParameters={}):ProfileGraphDraft {
  const previous=value.document.processingNodes.find(node=>node.id===nodeId);
  if(!previous?.draft)throw new Error('Choose an unresolved transformation to configure its preset.');
  let configured:ProcessingParameters=parameters;
  if(previous.draft.preset===preset){
    const defaults=createProcessingDraft(preset,nodeId).operation as unknown as Record<string,unknown>;
    configured={...Object.fromEntries(Object.entries(previous.operation).filter(([key,item])=>key!=='kind'&&((previous.draft?.configured??[]).includes(key)||!equal(item,defaults[key])))),...parameters};
  }
  const node=createProcessingDraft(preset,nodeId,configured);node.name=previous.draft.preset===preset?previous.name:node.name;
  return graphSetProcessing(value,node);
}

/** Keep user parameters configured before wiring, and validate any changed
 * resolved output against all downstream ports in one graph edit. */
export function graphUpdateProcessing(value:DeepReadonly<ProfileGraphDraft>,edited:DeepReadonly<ProfileProcessingNode>):ProfileGraphDraft {
  const previous=value.document.processingNodes.find(node=>node.id===edited.id),node=processingDerivedNode(edited);
  if(previous?.draft&&node.draft){
    const before=previous.operation as unknown as Record<string,unknown>,after=node.operation as unknown as Record<string,unknown>;
    const configured=new Set(node.draft.configured??[]);for(const [key,item] of Object.entries(after))if(key!=='kind'&&!equal(item,before[key]))configured.add(key);
    if(configured.size)node.draft.configured=[...configured];
  }
  const error=processingNodeError(node)||profileDocumentError({...value.document,processingNodes:[...value.document.processingNodes.filter(row=>row.id!==node.id),node]});if(error)throw new Error(error);
  const next=graphSetProcessing(value,node);
  for(const binding of next.document.bindings){const issue=graphConnectionError(next.document,binding.from,binding.to,binding.id);if(issue)throw new Error(issue);}
  return next;
}
