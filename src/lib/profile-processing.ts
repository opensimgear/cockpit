import { profileContractError, profileDocumentError, profilePortIdValid, profileScalarAccepts, type DeepReadonly, type ProfileActionParameter, type ProfileBuiltin, type ProfileOperation, type ProfilePort, type ProfilePortContract, type ProfileProcessingNode, type ProfileScalarSchema, type ProfileValue } from './profiles';
import { graphContractError, graphScalarFits } from './profile-graph';

export const PROCESSING_PRESETS = [
  ['identity','Identity'],['declare-quantity','Interpret raw quantity'],['not','Boolean NOT'],['invert','Invert numeric value'],['scale','Scale and offset'],['clamp','Clamp'],['range-map','Map numeric range'],['unit-convert','Convert units'],['ratio-to-percent','Ratio to percentage (0–100%)'],['deadzone','Deadzone'],['curve','Curve'],['compare','Compare'],['hysteresis','Condition with hysteresis'],['gate','Gate'],['any','Boolean OR'],['all','Boolean AND'],['priority','Explicit input priority'],['event-edge','State to event edge'],['event-to-state','Event to state'],['select','Choose values'],['format','Format text'],['fallback','Explicit unknown/stale fallback'],['constant','Constant value'],['toggle','Toggle'],['pulse','Pulse'],['tap','Tap'],['hold','Hold'],['double-tap','Double tap'],['macro','Timed macro'],['script','Script with declared ports'],
] as const;
export type ProcessingPreset = typeof PROCESSING_PRESETS[number][0];
type ScalarContract = Extract<ProfilePortContract,{delivery:'state'|'event'}>;
type Numeric = Extract<ProfileScalarSchema,{type:'number'|'integer'}>;
const clone=<T>(value:DeepReadonly<T>):T=>JSON.parse(JSON.stringify(value));
const bool:ProfileScalarSchema={type:'boolean'};
const state=(schema:ProfileScalarSchema):ScalarContract=>({delivery:'state',schema,freshnessMs:1000,fallback:null});
const scalar=(contract:DeepReadonly<ProfilePortContract>):DeepReadonly<ProfileScalarSchema>|null=>'schema' in contract?contract.schema:null;
const numeric=(schema:DeepReadonly<ProfileScalarSchema>|null):schema is DeepReadonly<Numeric>=>schema?.type==='number'||schema?.type==='integer';
const port=(id:string,direction:'input'|'output',contract:ProfilePortContract):ProfilePort=>({id,name:id,direction,contract});
const sample=(schema:DeepReadonly<ProfileScalarSchema>):Exclude<ProfileValue,null>=>schema.type==='boolean'?false:schema.type==='rgb'?'000000':schema.type==='text'?'':schema.type==='enum'?schema.values[0]:schema.min;
const conversionTable:Record<string,Record<string,[number,number]>>={m:{ft:[1250/381,0],km:[.001,0]},ft:{m:[381/1250,0]},km:{m:[1000,0]},s:{ms:[1000,0]},ms:{s:[.001,0]},'m/s':{kn:[900/463,0]},kn:{'m/s':[463/900,0]},deg:{rad:[Math.PI/180,0]},rad:{deg:[180/Math.PI,0]},degC:{degF:[1.8,32]},degF:{degC:[1/1.8,-32/1.8]},ratio:{'%':[100,0]},'%':{ratio:[.01,0]}};
export function processingUnitConversion(from:string,to:string):[number,number]|null{if(from===to)return[1,0];if(!Object.hasOwn(conversionTable,from)||!Object.hasOwn(conversionTable[from],to))return null;return[...conversionTable[from][to]];}
function boundedSummary(value:string):string{
  const encoder=new TextEncoder();if(encoder.encode(value).length<=255)return value;
  let result='',used=0;for(const character of value){const size=encoder.encode(character).length;if(used+size>252)break;result+=character;used+=size;}return `${result}…`;
}
function builtinSummary(rule:DeepReadonly<ProfileBuiltin>,inputs:readonly string[]):string{
  const time=(value:number)=>value>=1000?`${value/1000} ms`:`${value} µs`;
  let parameters:string;
  switch(rule.kind){
    case 'axis':parameters=`axis · deadzone ${rule.deadZone} · ${rule.invert?'inverted':'normal'} · scale ${rule.scale} · curve ${rule.curve.length?`${rule.curve.length} points [${rule.curve.map(point=>`${point.input}→${point.output}`).join(', ')}]`:'linear'}`;break;
    case 'split':parameters=`split · ${rule.positive?'positive':'negative'} half`;break;
    case 'button-axis':parameters=`button-axis · ${rule.range} · pressed ${rule.pressed}, released ${rule.released}`;break;
    case 'threshold':parameters=`threshold · press ${rule.press}, release ${rule.release}`;break;
    case 'gesture':parameters=`${rule.gesture} · hold ${time(rule.holdUs)} · double ${time(rule.doubleUs)} · pulse ${time(rule.pulseUs)}`;break;
    case 'pulse':parameters=`pulse · duration ${time(rule.durationUs)} · ${rule.repeatUs?`repeat ${time(rule.repeatUs)}`:'no repeat'}`;break;
    case 'macro':parameters=`macro · ${rule.steps.length} steps · duration ${time(rule.steps.at(-1)?.afterUs??0)} · ${rule.steps.map(step=>`${time(step.afterUs)} ${step.pressed?'on':'off'}`).join(' → ')}`;break;
    default:parameters=rule.kind;
  }
  return boundedSummary(`${parameters} · explicit inputs: ${inputs.join(', ')}`);
}
export function processingSummary(node:DeepReadonly<ProfileProcessingNode>):string{
  const op=node.operation;
  if(op.kind==='script')return `Script · ${node.inputs.length} declared inputs → ${node.outputs.length} outputs`;
  if(op.kind==='action-pack'||op.kind==='set-on-change')return `${op.kind==='action-pack'?'On explicit event':'On argument change'} · ${op.arguments.length} ordered arguments`;
  if(op.kind==='declare-quantity')return boundedSummary(`${op.quantity} · declared ${op.unit} · value unchanged`);
  if(op.kind==='unit-convert'){const output=node.outputs[0]?.contract,schema=output&&scalar(output);return `${op.from} → ${op.to} · ${op.rounding}${op.clamp&&numeric(schema)?` · clamp ${schema.min}–${schema.max}${schema.unit??''}`:''}`;}
  if(op.kind==='range-map')return `${op.inputMin}…${op.inputMax} → ${op.outputMin}…${op.outputMax} · ${op.rounding}`;
  if(op.kind==='scale')return `× ${op.factor} + ${op.offset}`;
  if(op.kind==='clamp')return `Clamp ${op.min}…${op.max}`;
  if(op.kind==='priority')return `Priority: ${op.order.join(' → ')}`;
  if(op.kind==='builtin')return builtinSummary(op.rule,op.inputs);
  return Object.entries(op).map(([key,value])=>key==='kind'?String(value):`${key}: ${Array.isArray(value)?`${value.length} entries`:String(value)}`).join(' · ');
}
/** Create a visible node only; connections are separate explicit graph edits. */
export function processingTemplate(preset:ProcessingPreset,id:string,input:DeepReadonly<ProfilePortContract>=state(bool),target?:DeepReadonly<ProfilePortContract>):ProfileProcessingNode{
  if(input.delivery!=='state'&&input.delivery!=='event')throw new Error('Choose a scalar state or event for this transformation. Actions use an explicit argument pack.');
  if(profileContractError(input,'output')||target&&profileContractError(target,target.delivery==='invocation'?'output':'input'))throw new Error('Choose valid declared input and output contracts.');
  const source=clone<ScalarContract>(input),schema=source.schema;
  const sameDelivery=(nextSchema:ProfileScalarSchema):ScalarContract=>source.delivery==='event'?{delivery:'event',schema:nextSchema}:{...source,schema:nextSchema,fallback:null};
  let operation:ProfileOperation={kind:'identity'},inputs=[port('in','input',source)],output:ProfilePortContract=clone<ScalarContract>(source);
  const requireNumeric=():Numeric=>{if(!numeric(schema))throw new Error('This transformation requires an integer or number port.');return schema;};
  const requireBoolean=()=>{if(schema.type!=='boolean')throw new Error('This transformation requires a boolean port.');};
  const requireState=()=>{if(source.delivery!=='state')throw new Error('This transformation requires state. Add an explicit Event to state node first.');};
  switch(preset){
    case 'identity':operation={kind:'identity'};break;
    case 'declare-quantity':{const from=requireNumeric();if(from.unit!==null)throw new Error('Interpret raw quantity requires a unitless source. Use Convert units for an existing declared unit.');operation={kind:'declare-quantity',quantity:'Raw count',unit:'count'};output=sameDelivery({...from,unit:'count'});break;}
    case 'not':requireBoolean();operation={kind:'not'};break;
    case 'invert':requireNumeric();operation={kind:'invert'};break;
    case 'scale':requireNumeric();operation={kind:'scale',factor:1,offset:0};break;
    case 'clamp':{const from=requireNumeric();operation={kind:'clamp',min:from.min,max:from.max};break;}
    case 'range-map':{const from=requireNumeric(),to=target&&numeric(scalar(target))?clone<Numeric>(scalar(target)! as DeepReadonly<Numeric>):from;operation={kind:'range-map',inputMin:from.min,inputMax:from.max,outputMin:to.min,outputMax:to.max,rounding:to.type==='integer'?'nearest':'none'};output=sameDelivery(to);break;}
    case 'unit-convert':{const from=requireNumeric();if(!from.unit)throw new Error('Unit conversion requires a declared source unit.');const to=target&&numeric(scalar(target))?clone<Numeric>(scalar(target)! as DeepReadonly<Numeric>):null;const unit=to?.unit??Object.keys(conversionTable[from.unit]??{})[0];const conversion=unit&&processingUnitConversion(from.unit,unit);if(!unit||!conversion)throw new Error('Choose a supported explicit unit conversion.');const [factor,offset]=conversion;const a=from.min*factor+offset,b=from.max*factor+offset;output=sameDelivery(to??{type:'number',min:Math.min(a,b),max:Math.max(a,b),unit});operation={kind:'unit-convert',from:from.unit,to:unit,rounding:to?.type==='integer'?'nearest':'none'};break;}
    case 'ratio-to-percent':{const from=requireNumeric();if(from.unit!=='ratio')throw new Error('Ratio to percentage requires a ratio source.');const to=target?scalar(target):null;if(target&&(!numeric(to)||to.type!=='number'||to.unit!=='%'||to.min>0||to.max<100))throw new Error('Ratio to percentage requires a number target accepting the full 0–100% range.');output=sameDelivery({type:'number',min:0,max:100,unit:'%'});operation={kind:'unit-convert',from:'ratio',to:'%',rounding:'none',clamp:true};break;}
    case 'deadzone':{const from=requireNumeric();operation={kind:'deadzone',center:from.min<=0&&from.max>=0?0:(from.min+from.max)/2,radius:0,rounding:from.type==='integer'?'nearest':'none'};break;}
    case 'curve':{const from=requireNumeric();operation={kind:'curve',points:[{input:from.min,output:from.min},{input:from.max,output:from.max}],rounding:from.type==='integer'?'nearest':'none'};break;}
    case 'compare':{const from=requireNumeric();operation={kind:'compare',comparison:'greater-equal',threshold:(from.min+from.max)/2};output=sameDelivery(bool);break;}
    case 'hysteresis':{const from=requireNumeric();operation={kind:'hysteresis',comparison:'greater-equal',press:from.min+(from.max-from.min)*.6,release:from.min+(from.max-from.min)*.4};output=sameDelivery(bool);break;}
    case 'gate':inputs=[port('gate','input',state(bool)),port('value','input',source)];operation={kind:'gate'};break;
    case 'any':case 'all':requireState();requireBoolean();inputs=[port('first','input',source),port('second','input',clone<ScalarContract>(source))];operation={kind:preset};break;
    case 'priority':requireState();inputs=[port('first','input',source),port('second','input',clone<ScalarContract>(source))];operation={kind:'priority',order:['first','second']};break;
    case 'event-edge':requireState();operation={kind:'event-edge',edge:schema.type==='boolean'?'rising':'change'};output={delivery:'event',schema:clone<ProfileScalarSchema>(schema)};break;
    case 'event-to-state':if(source.delivery!=='event')throw new Error('Event to state requires an event source.');operation={kind:'event-to-state'};output=state(clone<ProfileScalarSchema>(schema));break;
    case 'select':requireBoolean();{const to=target&&scalar(target)?clone<ProfileScalarSchema>(scalar(target)!):bool;operation={kind:'select',whenTrue:to.type==='boolean'?true:sample(to),whenFalse:sample(to)};output=sameDelivery(to);break;}
    case 'format':requireNumeric();operation={kind:'format',decimals:0,prefix:'',suffix:'',rounding:'nearest',overflow:'reject'};output=sameDelivery(target&&scalar(target)?.type==='text'?clone<ProfileScalarSchema>(scalar(target)!):{type:'text',maxBytes:256,encoding:'utf8'});break;
    case 'fallback':operation={kind:'fallback',unknown:sample(schema),stale:sample(schema),unavailable:sample(schema)};break;
    case 'constant':inputs=[];output=state(clone<ProfileScalarSchema>(schema));operation={kind:'constant',value:sample(schema)};break;
    case 'toggle':case 'pulse':case 'tap':case 'hold':case 'double-tap':case 'macro':{
      requireState();requireBoolean();const rule=preset==='toggle'?{kind:'toggle' as const}:preset==='pulse'?{kind:'pulse' as const,durationUs:100000,repeatUs:0}:preset==='macro'?{kind:'macro' as const,steps:[{afterUs:0,pressed:true},{afterUs:100000,pressed:false}]}:{kind:'gesture' as const,gesture:preset,holdUs:500000,doubleUs:250000,pulseUs:100000};operation={kind:'builtin',rule,inputs:['in']};break;
    }
    case 'script':operation={kind:'script',source:'fn on_start(ctx) { transaction(ctx.state, #{}, []) }\nfn on_event(ctx) { transaction(ctx.state, #{}, []) }',parameters:{}};break;
    default:throw new Error('Choose a supported visible transformation.');
  }
  const node=clone<ProfileProcessingNode>({id,name:PROCESSING_PRESETS.find(([kind])=>kind===preset)![1],operation,inputs,outputs:[port('out','output',output)]});
  const error=processingNodeError(node);if(error)throw new Error(error);return node;
}
/** Each native argument is its own explicit state input; the invocation is atomic. */
export function processingActionPack(id:string,action:DeepReadonly<ProfilePortContract>,mode:'action-pack'|'set-on-change',trigger:DeepReadonly<ProfilePortContract>={delivery:'event',schema:bool}):ProfileProcessingNode{
  if(action.delivery!=='action'||profileContractError(action,'input'))throw new Error('Choose an advertised action contract.');
  if(mode==='set-on-change'&&action.semantics!=='setter')throw new Error('Set on change is only available for setter actions. One-shot actions need an explicit event trigger.');
  if(mode==='action-pack'&&trigger.delivery!=='event')throw new Error('Action pack needs an event trigger; insert a visible State to event edge node first.');
  const used=new Set(action.parameters.map(parameter=>parameter.id));const fresh=(base:string)=>{let id=base;for(let n=1;used.has(id);n++)id=`${base}-${n}`;used.add(id);return id;};
  const inputs=action.parameters.map(parameter=>({...port(parameter.id,'input',state(clone<ProfileScalarSchema>(parameter.schema))),name:[...parameter.name.trim()].slice(0,80).join('')||parameter.id}));if(mode==='action-pack')inputs.unshift(port(fresh('fire'),'input',clone<ProfilePortContract>(trigger)));
  const output=port(fresh('invoke'),'output',{delivery:'invocation',parameters:clone<ProfileActionParameter[]>(action.parameters),semantics:action.semantics});
  const node:ProfileProcessingNode={id,name:mode==='action-pack'?'Action pack':'Set on change',operation:{kind:mode,arguments:action.parameters.map(parameter=>parameter.id)},inputs,outputs:[output]};
  const error=processingNodeError(node);if(error)throw new Error(error);return node;
}
const equalSchema=(a:DeepReadonly<ProfileScalarSchema>|null,b:DeepReadonly<ProfileScalarSchema>|null)=>!!a&&!!b&&graphScalarFits(a,b)&&graphScalarFits(b,a)&&(a.type!=='enum'||b.type==='enum'&&a.values.every((value,index)=>value===b.values[index]));
const numericError=(input:DeepReadonly<ProfileScalarSchema>|null,output:DeepReadonly<ProfileScalarSchema>|null,factor:number,offset:number,rounding:string,units?:[string,string],clamp=false):string=>{
  if(!numeric(input)||!numeric(output))return 'Numeric conversion requires numeric input and output ports.';
  const round=(value:number)=>rounding==='nearest'?(value<0?-Math.round(-value):Math.round(value)):rounding==='floor'?Math.floor(value):rounding==='ceil'?Math.ceil(value):rounding==='truncate'?Math.trunc(value):value;
  const rawA=input.min*factor+offset,rawB=input.max*factor+offset;
  const bounded=(value:number)=>clamp?Math.min(output.max,Math.max(output.min,value)):value;
  const a=round(bounded(rawA)),b=round(bounded(rawB));
  if(![factor,offset,rawA,rawB,a,b].every(value=>Number.isFinite(value)&&Math.abs(value)<=Number.MAX_SAFE_INTEGER)||(units?input.unit!==units[0]||output.unit!==units[1]:input.unit!==output.unit)||Math.min(a,b)<output.min-1e-9||Math.max(a,b)>output.max+1e-9||output.type==='integer'&&rounding==='none'&&(input.type!=='integer'||!Number.isInteger(factor)||!Number.isInteger(offset)))return 'Conversion output range, rounding or units do not fit the declared ports.';
  return '';
};
const controllerKind=(schema:DeepReadonly<ProfileScalarSchema>|null):string|null=>schema?.type==='boolean'?'button':schema?.type==='integer'&&schema.unit===null?(schema.min===0&&schema.max===32767?'unipolar':schema.min===-32768&&schema.max===32767?'bipolar':schema.min===-1&&schema.max===7?'hat':null):null;
function builtinOutput(rule:DeepReadonly<ProfileBuiltin>,kinds:readonly(string|null)[]):string|null{
  const single=kinds.length===1,button=single&&kinds[0]==='button',axis=single&&(kinds[0]==='bipolar'||kinds[0]==='unipolar');
  const inRange=(value:number,kind:string|null)=>Number.isSafeInteger(value)&&value>=(kind==='bipolar'?-32768:0)&&value<=32767;
  const time=(value:number)=>Number.isInteger(value)&&value>=1000&&value<=60000000;
  switch(rule.kind){
    case 'axis':return axis&&rule.deadZone>=0&&rule.deadZone<32767&&rule.scale>=0&&rule.scale<=4000&&(!rule.curve.length||rule.curve.length>=2&&rule.curve.length<=32&&rule.curve[0].input===(kinds[0]==='bipolar'?-32768:0)&&rule.curve.at(-1)!.input===32767&&rule.curve.every((point,index)=>inRange(point.input,kinds[0])&&inRange(point.output,kinds[0])&&(!index||point.input>rule.curve[index-1].input)))?kinds[0]:null;
    case 'merge':return kinds.length===2&&kinds.every(kind=>kind==='unipolar')?'bipolar':null;
    case 'split':return single&&kinds[0]==='bipolar'?'unipolar':null;
    case 'button-axis':return button&&inRange(rule.pressed,rule.range)&&inRange(rule.released,rule.range)?rule.range:null;
    case 'threshold':return axis&&rule.release<rule.press&&inRange(rule.press,kinds[0])&&inRange(rule.release,kinds[0])?'button':null;
    case 'chord':return kinds.length>=2&&kinds.length<=8&&kinds.every(kind=>kind==='button')?'button':null;
    case 'gate':return kinds.length===2&&kinds[1]==='button'?kinds[0]:null;
    case 'toggle':return button?'button':null;
    case 'gesture':return button&&[rule.holdUs,rule.doubleUs,rule.pulseUs].every(time)?'button':null;
    case 'pulse':return button&&time(rule.durationUs)&&(rule.repeatUs===0||time(rule.repeatUs)&&rule.repeatUs>rule.durationUs)?'button':null;
    case 'macro':return button&&rule.steps.length>0&&rule.steps.length<=16&&rule.steps[0].afterUs===0&&!rule.steps.at(-1)!.pressed&&rule.steps.at(-1)!.afterUs<=60000000&&rule.steps.every((step,index)=>!index||step.afterUs>rule.steps[index-1].afterUs)?'button':null;
  }
}
/** Fast authoring checks; native compilation remains authoritative for the complete graph. */
export function processingNodeError(node:DeepReadonly<ProfileProcessingNode>):string{
  const shape=profileDocumentError({schemaVersion:2,id:'00000000-0000-4000-8000-000000000001',name:'Operation validation',revision:1,eligibility:{gameIds:[]},resources:[],bindings:[],processingNodes:[node],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}});if(shape)return shape;
  const fail='The operation does not match its declared ports, values or parameters.';
  if(node.inputs.length>128||!node.outputs.length||node.outputs.length>16||new Set([...node.inputs,...node.outputs].map(port=>port.id)).size!==node.inputs.length+node.outputs.length||node.inputs.some(port=>port.direction!=='input')||node.outputs.some(port=>port.direction!=='output')||[...node.inputs,...node.outputs].some(port=>!profilePortIdValid(port.id)||profileContractError(port.contract,port.direction)))return fail;
  const op=node.operation,input=node.inputs[0]?.contract,output=node.outputs[0].contract,a=input&&scalar(input),b=scalar(output),one=node.inputs.length===1&&node.outputs.length===1;
  const sameDelivery=one&&input.delivery===output.delivery,same=sameDelivery&&!graphContractError(input,output)&&!graphContractError(output,input);
  const booleanInputs=node.inputs.every(port=>scalar(port.contract)?.type==='boolean'),stateInputs=node.inputs.every(port=>port.contract.delivery==='state');
  const order=(ids:readonly string[])=>ids.length===node.inputs.length&&new Set(ids).size===ids.length&&node.inputs.every(port=>ids.includes(port.id));
  switch(op.kind){
    case 'hat-direction':return one&&input.delivery==='state'&&a?.type==='integer'&&a.min===-1&&a.max===7&&a.unit===null&&output.delivery==='state'&&b?.type==='boolean'?'':fail;
    case 'constant':return !node.inputs.length&&node.outputs.length===1&&output.delivery==='state'&&!!b&&profileScalarAccepts(b as ProfileScalarSchema,op.value)?'':fail;
    case 'identity':return same?'':fail;
    case 'declare-quantity':return sameDelivery&&numeric(a)&&numeric(b)&&a.unit===null&&a.type===b.type&&a.min===b.min&&a.max===b.max&&b.unit===op.unit?'':'Interpret raw quantity requires one unitless numeric input and the same output type, bounds and delivery with exactly the declared unit.';
    case 'not':return same&&booleanInputs?'':fail;
    case 'invert':return same&&numeric(a)?'':fail;
    case 'event-to-state':return one&&input.delivery==='event'&&output.delivery==='state'&&equalSchema(a,b)?'':fail;
    case 'event-edge':return one&&input.delivery==='state'&&output.delivery==='event'&&equalSchema(a,b)&&(op.edge==='change'||booleanInputs)?'':fail;
    case 'any':case 'all':return node.inputs.length>=2&&node.inputs.length<=8&&node.outputs.length===1&&stateInputs&&booleanInputs&&output.delivery==='state'&&b?.type==='boolean'?'':fail;
    case 'priority':return node.inputs.length>=2&&node.inputs.length<=8&&node.outputs.length===1&&stateInputs&&output.delivery==='state'&&node.inputs.every(port=>equalSchema(scalar(port.contract),b))&&order(op.order)?'':fail;
    case 'gate':return node.inputs.length===2&&node.outputs.length===1&&node.inputs.some(port=>port.id==='gate'&&port.contract.delivery==='state'&&scalar(port.contract)?.type==='boolean')&&node.inputs.some(port=>port.id==='value'&&!graphContractError(port.contract,output)&&!graphContractError(output,port.contract))?'':fail;
    case 'fallback':return same&&!!b&&[op.unknown,op.stale,op.unavailable].every(value=>profileScalarAccepts(b as ProfileScalarSchema,value))?'':fail;
    case 'select':return sameDelivery&&booleanInputs&&!!b&&[op.whenTrue,op.whenFalse].every(value=>profileScalarAccepts(b as ProfileScalarSchema,value))?'':fail;
    case 'scale':return sameDelivery?numericError(a,b,op.factor,op.offset,'none'):fail;
    case 'clamp':return sameDelivery&&numeric(a)&&numeric(b)&&a.type===b.type&&a.unit===b.unit&&op.min<=op.max&&Number.isFinite(op.min)&&Number.isFinite(op.max)&&(a.type!=='integer'||Number.isSafeInteger(op.min)&&Number.isSafeInteger(op.max))&&b.min===Math.min(op.max,Math.max(op.min,a.min))&&b.max===Math.min(op.max,Math.max(op.min,a.max))?'':fail;
    case 'range-map':return sameDelivery&&numeric(a)&&op.inputMin<op.inputMax&&a.min>=op.inputMin&&a.max<=op.inputMax?numericError(a,b,(op.outputMax-op.outputMin)/(op.inputMax-op.inputMin),op.outputMin-op.inputMin*(op.outputMax-op.outputMin)/(op.inputMax-op.inputMin),op.rounding):fail;
    case 'unit-convert':{const conversion=processingUnitConversion(op.from,op.to);return sameDelivery&&conversion?numericError(a,b,...conversion,op.rounding,[op.from,op.to],op.clamp):fail;}
    case 'deadzone':return same&&numeric(a)&&Number.isFinite(op.center)&&Number.isFinite(op.radius)&&op.radius>=0&&op.center>=a.min&&op.center<=a.max&&op.radius<=Math.max(a.max-op.center,op.center-a.min)&&(b?.type!=='integer'||op.rounding!=='none'||op.radius===0)?'':fail;
    case 'curve':return sameDelivery&&numeric(a)&&numeric(b)&&a.unit===b.unit&&op.points.length>=2&&op.points.length<=32&&a.min>=op.points[0].input&&a.max<=op.points[op.points.length-1].input&&op.points.every((point,index)=>Number.isFinite(point.input)&&Number.isFinite(point.output)&&point.output>=b.min&&point.output<=b.max&&(index===0||point.input>op.points[index-1].input))&&(b.type!=='integer'||op.rounding!=='none')?'':fail;
    case 'compare':return sameDelivery&&numeric(a)&&b?.type==='boolean'&&Number.isFinite(op.threshold)?'':fail;
    case 'hysteresis':return sameDelivery&&numeric(a)&&b?.type==='boolean'&&Number.isFinite(op.press)&&Number.isFinite(op.release)&&(op.comparison==='greater-equal'&&op.release<op.press||op.comparison==='less-equal'&&op.release>op.press)?'':fail;
    case 'format':return sameDelivery&&numeric(a)&&b?.type==='text'&&Number.isInteger(op.decimals)&&op.decimals>=0&&op.decimals<=6&&[op.prefix,op.suffix].every(value=>new TextEncoder().encode(value).length<=64&&!/[\u0000-\u001f\u007f-\u009f]/.test(value))?'':fail;
    case 'action-pack':case 'set-on-change':{
      if(node.outputs.length!==1||output.delivery!=='invocation'||op.arguments.length>4||op.arguments.length!==output.parameters.length||new Set(op.arguments).size!==op.arguments.length||node.inputs.length!==op.arguments.length+(op.kind==='action-pack'?1:0)||op.kind==='set-on-change'&&output.semantics!=='setter')return fail;
      for(let i=0;i<op.arguments.length;i++){const port=node.inputs.find(port=>port.id===op.arguments[i]),parameter=output.parameters[i];if(!port||parameter.id!==port.id||port.contract.delivery!=='state'||!equalSchema(scalar(port.contract),parameter.schema))return fail;}
      return op.kind==='set-on-change'||node.inputs.some(port=>!op.arguments.includes(port.id)&&port.contract.delivery==='event')?'':fail;
    }
    case 'script':return node.inputs.length>=1&&node.inputs.length<=32&&new TextEncoder().encode(op.source).length>0&&new TextEncoder().encode(op.source).length<=16384&&Object.keys(op.parameters).length<=64&&new TextEncoder().encode(JSON.stringify(op.parameters)).length<=4096?'':fail;
    case 'builtin':{const kinds=op.inputs.map(id=>controllerKind(scalar(node.inputs.find(port=>port.id===id)?.contract??output))),expected=builtinOutput(op.rule,kinds);return stateInputs&&node.outputs.length===1&&output.delivery==='state'&&order(op.inputs)&&expected!==null&&expected===controllerKind(b)?'':fail;}
  }
}
