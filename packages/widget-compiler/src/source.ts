import { lowerWidgetDesign } from './design.ts';
import { BEHAVIOR_LIMITS, evaluateExpression, validateBehaviorDefinition, type Expression, type Scalar } from '../../widget-runtime/src/behavior.ts';
import { canonicalWidget, decodeWidgetBase64, validatePcmWave } from '../../widget-runtime/src/artifact.ts';
import type { CompileOptions, VisualNode, VisualValue, WidgetParameter, WidgetSource } from './types.ts';

export const SOURCE_LIMITS = Object.freeze({ bytes: 1024 * 1024, nodes: 128, depth: 8, parameters: 32, domains: 16, combinations: 4096, motionFrames: 600 });
export function assertSource(test: unknown, message: string): asserts test { if (!test) throw new Error(message); }
export function exactSource(value: unknown, required: string[], optional: string[] = []): Record<string, any> {
  assertSource(value && typeof value === 'object' && !Array.isArray(value) && Object.getPrototypeOf(value) === Object.prototype, 'Expected plain widget source object');
  const v=value as Record<string,any>;assertSource(required.every(key=>Object.hasOwn(v,key))&&Object.keys(v).every(key=>required.includes(key)||optional.includes(key)),'Unexpected or missing widget source field');return v;
}
export function sourceNumber(value: unknown, min: number, max: number, integer = false): asserts value is number { assertSource(typeof value==='number' && Number.isFinite(value) && value>=min && value<=max && (!integer || Number.isSafeInteger(value)), 'Widget source number outside bounds'); }
export function sourceId(value: unknown): asserts value is string { assertSource(typeof value==='string'&&/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value),'Invalid widget source ID'); }
function sourceList(value: unknown,max:number):any[]{assertSource(Array.isArray(value)&&value.length<=max,'Widget source list exceeds limit');return value;}
function sourceText(value: unknown,max=120):asserts value is string{assertSource(typeof value==='string'&&value.length<=max&&!/[\u0000-\u001f]/.test(value),'Invalid widget source text');}
export function resolvedParameters(source:WidgetSource, overrides:Record<string,Scalar>={}):Record<string,Scalar>{
  exactSource(overrides,[],source.parameters.map(parameter=>parameter.id));return Object.fromEntries(source.parameters.map(parameter=>{const value=overrides[parameter.id]??parameter.default;validateParameter(parameter,value);return [parameter.id,value];}));
}
function validateParameter(parameter:WidgetParameter,value:Scalar):void{
  if(parameter.type==='number')sourceNumber(value,parameter.min,parameter.max);
  else if(parameter.type==='boolean')assertSource(typeof value==='boolean','Expected boolean parameter');
  else{sourceText(value);if(parameter.type==='color')assertSource(/^#[a-fA-F0-9]{6}([a-fA-F0-9]{2})?$/.test(String(value)),'Expected hexadecimal parameter color');}
}
export function expressionRefs(expression:unknown, refs=new Set<string>(), regions=new Set<string>(),depth=0):{refs:Set<string>;regions:Set<string>}{
  if(expression===null || typeof expression!=='object')return {refs,regions};assertSource(depth<SOURCE_LIMITS.depth+BEHAVIOR_LIMITS.expressionDepth+3,'Visual reference traversal depth exceeded');const expr=expression as Record<string,any>;
  if(Object.hasOwn(expr,'ref'))refs.add(expr.ref);else if(Object.hasOwn(expr,'state'))regions.add(expr.state);
  else for(const value of Object.values(expr))if(Array.isArray(value))for(const item of value)expressionRefs(item,refs,regions,depth+1);else if(value&&typeof value==='object')expressionRefs(value,refs,regions,depth+1);
  return {refs,regions};
}
function validateValue(value:unknown,parameters:Set<string>,signals:Set<string>,regions:Set<string>,depth=0,budget={nodes:0}):void{
  if(depth===0){if(typeof value==='string'){sourceText(value,4096);return;}if(typeof value==='number'){sourceNumber(value,-1e9,1e9);return;}if(typeof value==='boolean')return;}
  assertSource(depth<BEHAVIOR_LIMITS.expressionDepth&&++budget.nodes<=BEHAVIOR_LIMITS.expressionNodes,'Visual expression exceeds work bounds');const v=value as Record<string,any>;assertSource(v&&typeof v==='object'&&!Array.isArray(v),'Invalid visual value');
  const child=(value:unknown)=>validateValue(value,parameters,signals,regions,depth+1,budget);
  if(Object.hasOwn(v,'literal')){exactSource(v,['literal']);assertSource(['string','boolean','number'].includes(typeof v.literal),'Invalid expression literal');if(typeof v.literal==='string')sourceText(v.literal);else if(typeof v.literal==='number')sourceNumber(v.literal,-1e9,1e9);}
  else if(Object.hasOwn(v,'ref')){exactSource(v,['ref']);assertSource(parameters.has(v.ref)||signals.has(v.ref),'Unknown visual value reference');}
  else if(Object.hasOwn(v,'state')){exactSource(v,['state']);assertSource(regions.has(v.state),'Unknown visual region');}
  else if(v.op==='not'){exactSource(v,['op','value']);child(v.value);}
  else if(v.op==='and'||v.op==='or'){exactSource(v,['op','values']);assertSource(sourceList(v.values,16).length>0,'Empty expression operands');for(const value of v.values)child(value);}
  else if(['eq','ne','lt','lte','gt','gte','add','sub','mul','div'].includes(v.op)){exactSource(v,['op','left','right']);child(v.left);child(v.right);}
  else if(v.op==='clamp'){exactSource(v,['op','value','min','max']);for(const key of ['value','min','max'])child(v[key]);}
  else{assertSource(v.op==='if','Unsupported visual expression');exactSource(v,['op','condition','then','else']);for(const key of ['condition','then','else'])child(v[key]);}
}
/** Single source schema. Authoring data is lowered before bounded runtime admission. */
export function validateWidgetSource(input:unknown):{ok:true;value:WidgetSource}|{ok:false;error:string}{
  try{
    const serialized=JSON.stringify(input);assertSource(serialized&&new TextEncoder().encode(serialized).length<=SOURCE_LIMITS.bytes,'Widget source exceeds byte budget');
    const source=exactSource(JSON.parse(serialized),['id','title','size','parameters','behavior','visualDomains','inputs','outputs','hitRegions','scene'],['design','motions','sounds','assets']) as WidgetSource;
    const checked=validateExecutableSource(lowerWidgetDesign(source));assertSource(checked.ok,checked.ok?'':checked.error);return {ok:true,value:source};
  }catch(e){return {ok:false,error:e instanceof Error?e.message:'Invalid widget source'};}
}
/** Strict replacement schema. Old widget libraries deliberately fail this validator. */
function validateExecutableSource(input:unknown):{ok:true;value:WidgetSource}|{ok:false;error:string}{
  try{
    const serialized=JSON.stringify(input);assertSource(serialized&&new TextEncoder().encode(serialized).length<=SOURCE_LIMITS.bytes,'Widget source exceeds byte budget');const s=exactSource(JSON.parse(serialized),['id','title','size','parameters','behavior','visualDomains','inputs','outputs','hitRegions','scene'],['motions','sounds','assets']);
    sourceId(s.id);sourceText(s.title);assertSource(s.title.trim(),'Widget title required');exactSource(s.size,['width','height']);sourceNumber(s.size.width,16,2048,true);sourceNumber(s.size.height,16,2048,true);
    const checked=validateBehaviorDefinition(s.behavior);assertSource(checked.ok,checked.ok?'':checked.error);const behavior=checked.value;const signals=new Set(behavior.signals.map(signal=>signal.id)),regions=new Set(behavior.regions.map(region=>region.id)),parameters=new Set<string>();
    for(const parameter of sourceList(s.parameters,SOURCE_LIMITS.parameters)){
      exactSource(parameter,['id','label','type','default'],['min','max']);sourceId(parameter.id);sourceText(parameter.label);assertSource(!parameters.has(parameter.id)&&!signals.has(parameter.id),'Duplicate parameter/signal ID');parameters.add(parameter.id);assertSource(['text','color','number','boolean'].includes(parameter.type),'Unsupported parameter type');if(parameter.type==='number'){sourceNumber(parameter.min,-1e9,1e9);sourceNumber(parameter.max,-1e9,1e9);assertSource(parameter.min<parameter.max,'Invalid parameter range');}else assertSource(parameter.min===undefined&&parameter.max===undefined,'Unexpected parameter range');validateParameter(parameter,parameter.default);
    }
    const domainSignals=new Set<string>();for(const domain of sourceList(s.visualDomains,SOURCE_LIMITS.domains)){
      exactSource(domain,['signal'],['values','min','max','samples','maxErrorPixels','travelPixels','smoothingMs']);const signal=behavior.signals.find(signal=>signal.id===domain.signal);assertSource(signal&&!domainSignals.has(domain.signal),'Invalid or duplicate visual domain');domainSignals.add(domain.signal);if(domain.smoothingMs!==undefined){sourceNumber(domain.smoothingMs,0,60000,true);assertSource(signal.type==='number','Only numeric domains support smoothing');}
      if(domain.values!==undefined){
        assertSource(sourceList(domain.values,4096).length>0&&new Set(domain.values.map(canonicalWidget)).size===domain.values.length,'Invalid finite visual values');
        for(const value of domain.values){if(signal.type==='boolean')assertSource(typeof value==='boolean','Incompatible boolean visual domain');else if(signal.type==='enum')assertSource(signal.values.includes(value),'Incompatible enum visual domain');else sourceNumber(value,signal.min,signal.max);}
        assertSource(domain.min===undefined&&domain.max===undefined&&domain.samples===undefined,'Ambiguous visual domain');
        if(signal.type==='boolean')assertSource(domain.values.length===2,'Boolean visual domain must cover both values');
        else if(signal.type==='enum')assertSource(domain.values.length===signal.values.length,'Enum visual domain must cover every value');
        else{assertSource(domain.values.includes(signal.min)&&domain.values.includes(signal.max),'Numeric visual samples must cover signal bounds');sourceNumber(domain.maxErrorPixels,0,2048);sourceNumber(domain.travelPixels,0,1e6);}
      }
      else{assertSource(signal.type==='number','Only numeric signals require sampling');assertSource(domain.min===signal.min&&domain.max===signal.max,'Visual sample range must cover signal bounds');sourceNumber(domain.samples,2,4096,true);sourceNumber(domain.maxErrorPixels,0,2048);sourceNumber(domain.travelPixels,0,1e6);}
    }
    for(const direction of ['inputs','outputs']){const ids=new Set<string>();for(const port of sourceList(s[direction],16)){
      exactSource(port,['id','label','type','signal'],['min','max','values']);sourceId(port.id);sourceText(port.label);
      const signal=behavior.signals.find(signal=>signal.id===port.signal);assertSource(signal&&signal.type===port.type&&!ids.has(port.id),'Invalid or duplicate widget port');ids.add(port.id);
      if(signal.type==='number')assertSource(port.min===signal.min&&port.max===signal.max&&port.values===undefined,'Port numeric domain must match signal bounds');
      else if(signal.type==='enum')assertSource(canonicalWidget(port.values)===canonicalWidget(signal.values)&&port.min===undefined&&port.max===undefined,'Port enum must match signal values');
      else assertSource(port.min===undefined&&port.max===undefined&&port.values===undefined,'Unexpected boolean port domain');
      if(direction==='inputs'){const output=behavior.outputs?.find(output=>output.id===port.id);assertSource(output&&output.type===signal.type&&output.neutral===signal.initial,'Input port requires matching declared emit output and neutral');if(output.type==='number'&&signal.type==='number')assertSource(output.min===signal.min&&output.max===signal.max,'Input output range mismatch');if(output.type==='enum'&&signal.type==='enum')assertSource(canonicalWidget(output.values)===canonicalWidget(signal.values),'Input output enum mismatch');}
    }}
    const eventIds=new Set(behavior.events.map(event=>event.id)),hitIds=new Set();for(const hit of sourceList(s.hitRegions,32)){exactSource(hit,['id','x','y','width','height','events']);sourceId(hit.id);assertSource(!hitIds.has(hit.id),'Duplicate hit ID');hitIds.add(hit.id);sourceNumber(hit.x,0,s.size.width);sourceNumber(hit.y,0,s.size.height);sourceNumber(hit.width,1,s.size.width);sourceNumber(hit.height,1,s.size.height);assertSource(hit.x+hit.width<=s.size.width&&hit.y+hit.height<=s.size.height,'Hit region outside source surface');const events=exactSource(hit.events,[],['press','release','activate','cancel']);assertSource(Object.values(events).every(event=>eventIds.has(event)),'Unknown hit event');}
    const assets=new Set<string>();for(const asset of sourceList(s.assets??[],32)){exactSource(asset,['id','mime','base64']);sourceId(asset.id);assertSource(!assets.has(asset.id),'Duplicate source asset');assets.add(asset.id);const bytes=decodeWidgetBase64(asset.base64);if(asset.mime==='audio/wav')validatePcmWave(bytes);else imageDimensions(bytes,asset.mime);}
    let nodeCount=0;const nodeIds=new Set<string>();function nodes(list:unknown,depth:number):void{assertSource(depth<=SOURCE_LIMITS.depth,'Visual group nesting exceeds limit');for(const node of sourceList(list,SOURCE_LIMITS.nodes)){
      assertSource(++nodeCount<=SOURCE_LIMITS.nodes,'Too many visual nodes');const fields=node.kind==='group'?['children']:node.kind==='rectangle'?['x','y','width','height']:node.kind==='ellipse'||node.kind==='image'?['x','y','width','height',...(node.kind==='image'?['assetId']:[])]:node.kind==='line'?['x','y','x2','y2']:node.kind==='path'?['d']:node.kind==='text'?['x','y','text','font','fontSize']:[];
      assertSource(fields.length>0,'Unsupported visual primitive');exactSource(node,['id','kind',...fields],['label','visible','opacity','fill','stroke','strokeWidth','transform',...(node.kind==='rectangle'?['radius']:[]),...(node.kind==='text'?['align']:[])]);sourceId(node.id);assertSource(!nodeIds.has(node.id),'Duplicate visual node');nodeIds.add(node.id);if(node.label!==undefined)sourceText(node.label);
      for(const [key,value] of Object.entries(node))if(!['id','label','kind','children','transform','assetId','font','align'].includes(key))validateValue(value,parameters,signals,regions);
      if(node.transform!==undefined){const transform=exactSource(node.transform,[],['translateX','translateY','rotate','scaleX','scaleY','originX','originY']);for(const value of Object.values(transform))validateValue(value,parameters,signals,regions);}
      if(node.kind==='image')assertSource(assets.has(node.assetId)&&s.assets.find((asset:any)=>asset.id===node.assetId).mime.startsWith('image/'),'Missing image asset');if(node.kind==='text')assertSource(['sans','mono'].includes(node.font)&&[undefined,'left','center','right'].includes(node.align),'Unsupported source typography');if(node.kind==='group')nodes(node.children,depth+1);
    }}nodes(s.scene,0);assertSource(nodeCount>0,'Widget source needs artwork');
    const motions=new Set();for(const motion of sourceList(s.motions??[],32)){
      exactSource(motion,['id','from','to','durationMs','fps','tracks'],['interruption','fromRegions','toRegions']);sourceId(motion.id);assertSource(!motions.has(motion.id),'Duplicate source motion');motions.add(motion.id);sourceNumber(motion.durationMs,1,60000,true);sourceNumber(motion.fps,1,60,true);assertSource(Math.ceil(motion.durationMs*motion.fps/1000)+1<=SOURCE_LIMITS.motionFrames,'Motion exceeds baked frame limit');assertSource([undefined,'reverse','restart','snap'].includes(motion.interruption),'Invalid motion interruption');
      for(const [selector,regionSelector] of [[motion.from,motion.fromRegions??{}],[motion.to,motion.toRegions??{}]]){
        exactSource(selector,[],[...signals]);exactSource(regionSelector,[],[...regions]);assertSource(Object.keys(selector).length+Object.keys(regionSelector).length>0,'Motion selector must be explicit');
        for(const [signalId,value] of Object.entries(selector)){const signal=behavior.signals.find(signal=>signal.id===signalId)!;assertSource(signal.type==='boolean'?typeof value==='boolean':signal.type==='enum'?signal.values.includes(String(value)):typeof value==='number'&&value>=signal.min&&value<=signal.max,'Motion selector outside signal domain');}
        for(const [regionId,value] of Object.entries(regionSelector))assertSource(behavior.regions.find(region=>region.id===regionId)!.states.includes(String(value)),'Motion selector outside region domain');
      }
      for(const track of sourceList(motion.tracks,64)){exactSource(track,['node','property','keyframes'],['easing']);assertSource(nodeIds.has(track.node)&&/^(x|y|x2|y2|width|height|radius|opacity|strokeWidth|fontSize|transform\.(translateX|translateY|rotate|scaleX|scaleY|originX|originY))$/.test(track.property),'Invalid baked track property');assertSource([undefined,'linear','ease-in','ease-out','ease-in-out'].includes(track.easing),'Unsupported easing');const keyframes=sourceList(track.keyframes,32);assertSource(keyframes.length>=2,'Track requires keyframes');keyframes.forEach((keyframe,index)=>{exactSource(keyframe,['at','value']);sourceNumber(keyframe.at,0,1);sourceNumber(keyframe.value,-1e6,1e6);assertSource(index===0?keyframe.at===0:keyframe.at>keyframes[index-1].at,'Keyframe times must increase from zero');});assertSource(keyframes.at(-1).at===1,'Track requires final keyframe');}
    }
    const cueIds=new Set();for(const sound of sourceList(s.sounds??[],32)){exactSource(sound,['id','cue','assetId'],['cooldownMs','stopOnExit','volume']);sourceId(sound.id);sourceId(sound.cue);assertSource(!cueIds.has(sound.id),'Duplicate sound ID');cueIds.add(sound.id);assertSource(s.assets?.some((asset:any)=>asset.id===sound.assetId&&asset.mime==='audio/wav'),'Missing sound asset');if(sound.cooldownMs!==undefined)sourceNumber(sound.cooldownMs,0,60000,true);if(sound.volume!==undefined)sourceNumber(sound.volume,0,1);if(sound.stopOnExit!==undefined)assertSource(typeof sound.stopOnExit==='boolean','Invalid cue exit policy');}
    return {ok:true,value:s as WidgetSource};
  }catch(e){return {ok:false,error:e instanceof Error?e.message:'Invalid generic widget source'};}
}
export function pngCrc32(bytes:Uint8Array):number{let crc=0xffffffff;for(const byte of bytes){crc^=byte;for(let bit=0;bit<8;bit++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);}return (crc^0xffffffff)>>>0;}
export function imageDimensions(bytes:Uint8Array,mime:string):{width:number;height:number}{
  assertSource(bytes.length>0&&bytes.length<=2*1024*1024,'Source image exceeds byte budget');
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),ascii=(offset:number)=>String.fromCharCode(...bytes.subarray(offset,offset+4));let width=0,height=0;
  if(mime==='image/png'){
    assertSource(bytes.length>=33&&[137,80,78,71,13,10,26,10].every((byte,index)=>bytes[index]===byte)&&view.getUint32(8)===13&&ascii(12)==='IHDR','Invalid PNG image');width=view.getUint32(16);height=view.getUint32(20);
    let cursor=8,data=false,ended=false;while(cursor+12<=bytes.length){const size=view.getUint32(cursor),kind=ascii(cursor+4),end=cursor+size+12;assertSource(end<=bytes.length,'Truncated PNG chunk');assertSource(pngCrc32(bytes.subarray(cursor+4,end-4))===view.getUint32(end-4),'PNG chunk CRC mismatch');assertSource(!['acTL','fcTL','fdAT'].includes(kind),'Animated source images unsupported');if(kind==='IDAT')data=true;if(kind==='IEND'){assertSource(size===0&&end===bytes.length,'Invalid PNG end');ended=true;break;}cursor=end;}assertSource(data&&ended,'PNG data/end is missing');
  }
  else if(mime==='image/jpeg'){
    assertSource(bytes.length>=12&&bytes[0]===255&&bytes[1]===216&&bytes.at(-2)===255&&bytes.at(-1)===217,'Invalid JPEG image');let cursor=2,frames=0;
    while(cursor+4<=bytes.length){assertSource(bytes[cursor]===255,'Invalid JPEG marker');while(bytes[cursor]===255)cursor++;const marker=bytes[cursor++];if(marker===218||marker===217)break;const size=view.getUint16(cursor);assertSource(size>=2&&cursor+size<=bytes.length,'Truncated JPEG');if([192,193,194].includes(marker)){assertSource(++frames===1&&size>=8,'Multiple or invalid JPEG frame headers');height=view.getUint16(cursor+3);width=view.getUint16(cursor+5);}cursor+=size;}
  }
  else if(mime==='image/webp'){
    assertSource(bytes.length>=30&&ascii(0)==='RIFF'&&ascii(8)==='WEBP'&&view.getUint32(4,true)+8===bytes.length,'Invalid WebP image');let cursor=12;const frameSizes:{width:number;height:number}[]=[];
    while(cursor+8<=bytes.length){const kind=ascii(cursor),size=view.getUint32(cursor+4,true),end=cursor+8+size+(size%2),start=cursor+8;assertSource(!['ANIM','ANMF'].includes(kind),'Animated source images unsupported');assertSource(end<=bytes.length,'Truncated WebP chunk');
      if(kind==='VP8 '){assertSource(size>=10&&bytes[start+3]===157&&bytes[start+4]===1&&bytes[start+5]===42,'Invalid WebP frame');frameSizes.push({width:view.getUint16(start+6,true)&16383,height:view.getUint16(start+8,true)&16383});}
      if(kind==='VP8L'){assertSource(size>=5&&bytes[start]===47,'Invalid WebP lossless frame');frameSizes.push({width:1+bytes[start+1]+((bytes[start+2]&63)<<8),height:1+(bytes[start+2]>>6)+(bytes[start+3]<<2)+((bytes[start+4]&15)<<10)});}
      cursor=end;
    }assertSource(cursor===bytes.length&&frameSizes.length<=1,'Invalid static WebP frame closure');
    const type=ascii(12);if(type==='VP8X'){assertSource((bytes[20]&2)===0,'Animated source images unsupported');width=1+bytes[24]+(bytes[25]<<8)+(bytes[26]<<16);height=1+bytes[27]+(bytes[28]<<8)+(bytes[29]<<16);}else if(type==='VP8L'){assertSource(bytes[20]===47,'Invalid WebP lossless frame');width=1+bytes[21]+((bytes[22]&63)<<8);height=1+(bytes[22]>>6)+(bytes[23]<<2)+((bytes[24]&15)<<10);}else{assertSource(type==='VP8 '&&bytes[23]===157&&bytes[24]===1&&bytes[25]===42,'Invalid WebP frame');width=view.getUint16(26,true)&16383;height=view.getUint16(28,true)&16383;}assertSource(frameSizes.every(frame=>frame.width===width&&frame.height===height),'WebP frame dimensions differ from canvas');
  }
  else throw new Error('Unsupported source image media');sourceNumber(width,1,4096,true);sourceNumber(height,1,4096,true);assertSource(width*height<=4*1024*1024,'Decoded source image exceeds budget');return {width,height};
}
export interface SceneContext { values:Record<string,Scalar>; regions:Record<string,string>; parameters?:Record<string,Scalar>; width?:number;height?:number }
export function resolveVisualValue(value:VisualValue|undefined,context:SceneContext,fallback:Scalar):Scalar{if(value===undefined)return fallback;return typeof value==='object'?evaluateExpression(value as Expression,{values:{...context.parameters,...context.values},regions:context.regions}):value;}
const escape=(value:unknown)=>String(value).replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&apos;'}[char]!));
/** Authoring-only projection. SVG is consumed by the pinned compiler, never deployed. */
export function sourceToSvg(source:WidgetSource,context:SceneContext):string{
  const width=context.width??source.size.width,height=context.height??source.size.height;
  let imageBytes=0;function imageBudget(scene:VisualNode[]):void{for(const node of scene){if(node.kind==='image'){imageBytes+=source.assets?.find(asset=>asset.id===node.assetId)?.base64.length??0;assertSource(imageBytes<=32*1024*1024,'Raster source embedding exceeds byte budget');}else if(node.kind==='group')imageBudget(node.children);}}imageBudget(source.scene);
  const number=(value:VisualValue|undefined,fallback=0)=>{const resolved=resolveVisualValue(value,context,fallback);sourceNumber(resolved,-1e6,1e6);return Number(resolved.toFixed(6));};
  const color=(value:VisualValue<string>|undefined,fallback:string)=>{const resolved=String(resolveVisualValue(value,context,fallback));assertSource(resolved==='none'||resolved==='transparent'||/^#[a-fA-F0-9]{6}([a-fA-F0-9]{2})?$/.test(resolved),'Unsupported resolved paint');return resolved;};
  function node(n:VisualNode):string{
    const visible=resolveVisualValue(n.visible,context,true);assertSource(typeof visible==='boolean','Visibility expression must return boolean');if(!visible)return '';
    const opacity=number(n.opacity,1);assertSource(opacity>=0&&opacity<=1,'Opacity outside bounds');let transform='';if(n.transform){const t=n.transform;transform=`translate(${number(t.translateX)} ${number(t.translateY)}) translate(${number(t.originX)} ${number(t.originY)}) rotate(${number(t.rotate)}) scale(${number(t.scaleX,1)} ${number(t.scaleY,1)}) translate(${-number(t.originX)} ${-number(t.originY)})`;}
    const attributes=`opacity="${opacity}" fill="${color(n.fill,'none')}" stroke="${color(n.stroke,'none')}" stroke-width="${number(n.strokeWidth)}"${transform?` transform="${transform}"`:''}`;
    if(n.kind==='group')return `<g ${attributes}>${n.children.map(node).join('')}</g>`;
    if(n.kind==='path'){const d=String(resolveVisualValue(n.d,context,''));assertSource(d.length<=4096&&/^[MmLlHhVvCcSsQqTtAaZz0-9eE+.,\s-]+$/.test(d),'Invalid resolved path');return `<path ${attributes} d="${escape(d)}"/>`;}
    if(n.kind==='line')return `<line ${attributes} x1="${number(n.x)}" y1="${number(n.y)}" x2="${number(n.x2)}" y2="${number(n.y2)}"/>`;
    if(n.kind==='text'){const text=String(resolveVisualValue(n.text,context,''));sourceText(text);const size=number(n.fontSize,16);sourceNumber(size,1,512);return `<text ${attributes} x="${number(n.x)}" y="${number(n.y)}" font-family="${n.font==='mono'?'IBM Plex Mono':'Rajdhani'}" font-size="${size}" font-weight="400" font-kerning="none" text-anchor="${n.align==='center'?'middle':n.align==='right'?'end':'start'}">${escape(text)}</text>`;}
    const x=number(n.x),y=number(n.y),w=number(n.width),h=number(n.height);sourceNumber(w,0,1e6);sourceNumber(h,0,1e6);
    if(n.kind==='rectangle')return `<rect ${attributes} x="${x}" y="${y}" width="${w}" height="${h}" rx="${number(n.radius)}"/>`;
    if(n.kind==='ellipse')return `<ellipse ${attributes} cx="${x+w/2}" cy="${y+h/2}" rx="${w/2}" ry="${h/2}"/>`;
    const asset=source.assets?.find(asset=>asset.id===n.assetId);assertSource(asset,'Missing raster source image');return `<image ${attributes} x="${x}" y="${y}" width="${w}" height="${h}" preserveAspectRatio="none" href="${escape(`data:${asset.mime};base64,${asset.base64}`)}"/>`;
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${source.size.width} ${source.size.height}" preserveAspectRatio="none">${source.scene.map(node).join('')}</svg>`;
}
export function checkedSource(source:WidgetSource,options:CompileOptions):WidgetSource{const checked=validateWidgetSource(source);assertSource(checked.ok,checked.ok?'':checked.error);resolvedParameters(checked.value,options.parameters);return lowerWidgetDesign(checked.value,options.parameters);}
