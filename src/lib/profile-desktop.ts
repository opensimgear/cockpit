import type { DeepReadonly, ProfilePortContract, ProfilePort, ProfileProcessingNode } from './profiles';

/** USB HID keyboard usages, never layout-dependent characters or virtual keys. */
export type DesktopMouseButton = 'left'|'right'|'middle'|'x1'|'x2';
export type DesktopAction = {kind:'key-hold';key:number}
  | {kind:'key-tap';key:number;downMs:number;upMs:number}
  | {kind:'chord';keys:number[]}
  | {kind:'mouse-hold';button:DesktopMouseButton}
  | {kind:'mouse-click';button:DesktopMouseButton;downMs:number;upMs:number}
  | {kind:'wheel';horizontal:number;vertical:number}
  | {kind:'pointer';axis:'x'|'y';speed:number;curve:number}
  | {kind:'repeat';key:number;delayMs:number;intervalMs:number};
/** The application reference resolves only through native, machine-local observation. */
export type ProfileDesktopScope = {kind:'selected-game'}|{kind:'application';applicationReference?:string}|{kind:'desktop-wide'};
export type ProfileDesktopOutput = {id:string;name:string;required:boolean;scope?:ProfileDesktopScope;ports:ProfilePort[];actions:{portId:string;action:DesktopAction}[]};
const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
const integer=(value:unknown,min:number,max:number)=>Number.isInteger(value)&&Number(value)>=min&&Number(value)<=max;
export const desktopKeyValid=(value:unknown):value is number=>integer(value,4,0x47)||integer(value,0x49,0x65)||integer(value,0xe0,0xe7);
const exact=(value:Record<string,unknown>,keys:string[])=>Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
export function desktopOutputScopeValid(value:unknown):value is ProfileDesktopScope {
  if(!object(value))return false;
  if(value.kind==='selected-game'||value.kind==='desktop-wide')return exact(value,['kind']);
  if(value.kind!=='application')return false;
  return Object.hasOwn(value,'applicationReference')?exact(value,['kind','applicationReference'])&&typeof value.applicationReference==='string'&&/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value.applicationReference):exact(value,['kind']);
}
export function desktopActionValid(value:unknown):value is DesktopAction {
  if(!object(value))return false;
  const key=desktopKeyValid(value.key),button=['left','right','middle','x1','x2'].includes(String(value.button));
  const dwell=integer(value.downMs,1,60_000)&&integer(value.upMs,1,60_000);
  switch(value.kind){
    case'key-hold':return exact(value,['kind','key'])&&key;
    case'key-tap':return exact(value,['kind','key','downMs','upMs'])&&key&&dwell;
    case'chord':return exact(value,['kind','keys'])&&Array.isArray(value.keys)&&value.keys.length>0&&value.keys.length<=8&&value.keys.every(desktopKeyValid)&&new Set(value.keys).size===value.keys.length;
    case'mouse-hold':return exact(value,['kind','button'])&&button;
    case'mouse-click':return exact(value,['kind','button','downMs','upMs'])&&button&&dwell;
    case'wheel':return exact(value,['kind','horizontal','vertical'])&&integer(value.horizontal,-120,120)&&integer(value.vertical,-120,120)&&(value.horizontal!==0||value.vertical!==0);
    case'pointer':return exact(value,['kind','axis','speed','curve'])&&['x','y'].includes(String(value.axis))&&integer(value.speed,1,20_000)&&typeof value.curve==='number'&&Number.isFinite(value.curve)&&value.curve>=0.1&&value.curve<=5;
    case'repeat':return exact(value,['kind','key','delayMs','intervalMs'])&&key&&integer(value.delayMs,1,60_000)&&integer(value.intervalMs,8,60_000);
    default:return false;
  }
}
export function desktopActionContract(action:DeepReadonly<DesktopAction>):ProfilePortContract {
  if(['key-tap','mouse-click','wheel'].includes(action.kind))return{delivery:'event',schema:{type:'boolean'}};
  return{delivery:'state',schema:action.kind==='pointer'?{type:'number',min:-1,max:1,unit:null}:{type:'boolean'},freshnessMs:1000,fallback:action.kind==='pointer'?0:false};
}
export function desktopActionPort(id:string,name:string,action:DeepReadonly<DesktopAction>):ProfilePort{return{id,name,direction:'input',contract:desktopActionContract(action)};}
export function desktopActionLabel(action:DeepReadonly<DesktopAction>):string {
  if('key'in action)return`${action.kind} · ${desktopKeyLabel(action.key)}`;
  if(action.kind==='chord')return action.keys.map(desktopKeyLabel).join(' + ');
  if('button'in action)return`${action.kind} · ${action.button}`;
  return action.kind==='wheel'?`Wheel ${action.horizontal}, ${action.vertical}`:`Pointer ${action.axis.toUpperCase()} · ${action.speed} px/s`;
}
export const DESKTOP_KEY_CODES:Readonly<Record<string,number>>=Object.freeze({
  ...Object.fromEntries(Array.from({length:26},(_,i)=>[`Key${String.fromCharCode(65+i)}`,4+i])),
  ...Object.fromEntries(Array.from({length:10},(_,i)=>[`Digit${(i+1)%10}`,30+i])),
  ...Object.fromEntries(Array.from({length:12},(_,i)=>[`F${i+1}`,58+i])),
  ...Object.fromEntries(Array.from({length:9},(_,i)=>[`Numpad${i+1}`,89+i])),
  Enter:40,Escape:41,Backspace:42,Tab:43,Space:44,Minus:45,Equal:46,BracketLeft:47,BracketRight:48,Backslash:49,Semicolon:51,Quote:52,Backquote:53,Comma:54,Period:55,Slash:56,CapsLock:57,PrintScreen:70,ScrollLock:71,
  Insert:73,Home:74,PageUp:75,Delete:76,End:77,PageDown:78,ArrowRight:79,ArrowLeft:80,ArrowDown:81,ArrowUp:82,
  NumLock:83,NumpadDivide:84,NumpadMultiply:85,NumpadSubtract:86,NumpadAdd:87,NumpadEnter:88,Numpad0:98,NumpadDecimal:99,IntlBackslash:100,ContextMenu:101,
  ControlLeft:224,ShiftLeft:225,AltLeft:226,MetaLeft:227,ControlRight:228,ShiftRight:229,AltRight:230,MetaRight:231,
});
const keyLabels:Record<string,string>={ArrowRight:'Right arrow',ArrowLeft:'Left arrow',ArrowDown:'Down arrow',ArrowUp:'Up arrow',ControlLeft:'Left Ctrl',ShiftLeft:'Left Shift',AltLeft:'Left Alt',MetaLeft:'Left GUI',ControlRight:'Right Ctrl',ShiftRight:'Right Shift',AltRight:'Right Alt',MetaRight:'Right GUI',IntlBackslash:'Non-US backslash'};
export const DESKTOP_KEYS:{usage:number;label:string}[]=[...Object.entries(DESKTOP_KEY_CODES).map(([code,usage])=>({usage,label:keyLabels[code]??code.replace(/^Key|^Digit/,'')})),{usage:50,label:'Non-US #'}].sort((a,b)=>a.usage-b.usage);
export function desktopKeyLabel(usage:number):string{return DESKTOP_KEYS.find(key=>key.usage===usage)?.label??`HID 0x${usage.toString(16).padStart(2,'0')}`;}
export type DesktopPreset='key-hold'|'encoder-tap'|'shortcut'|'mouse-hold'|'mouse-click'|'scroll'|'stick-pointer'|'hat-arrows';
export const DESKTOP_PRESETS:readonly[DesktopPreset,string][]=[['key-hold','Button → held key'],['encoder-tap','Encoder → serialized key taps'],['shortcut','Button → modifier shortcut'],['mouse-hold','Button → held mouse button'],['mouse-click','Button → mouse click'],['scroll','Button → scroll step'],['stick-pointer','Stick → pointer velocity'],['hat-arrows','Hat → arrow keys']];
/** Presets return declared actions plus visible processing; source selection stays explicit. */
export function desktopPreset(kind:DesktopPreset,id:string):{output:ProfileDesktopOutput;nodes:ProfileProcessingNode[];routes:{from:{nodeId:string;portId:string};to:{nodeId:string;portId:string}}[]} {
  let actions:ProfileDesktopOutput['actions'];const nodes:ProfileProcessingNode[]=[],routes:{from:{nodeId:string;portId:string};to:{nodeId:string;portId:string}}[]=[];
  if(kind==='hat-arrows')actions=[82,79,81,80].map((key,index)=>({portId:`arrow-${index}`,action:{kind:'key-hold',key}}));
  else if(kind==='stick-pointer')actions=[{portId:'pointer-x',action:{kind:'pointer',axis:'x',speed:900,curve:1}},{portId:'pointer-y',action:{kind:'pointer',axis:'y',speed:900,curve:1}}];
  else actions=[{portId:'action-1',action:kind==='encoder-tap'?{kind:'key-tap',key:44,downMs:30,upMs:30}:kind==='shortcut'?{kind:'chord',keys:[224,6]}:kind==='mouse-hold'?{kind:'mouse-hold',button:'left'}:kind==='mouse-click'?{kind:'mouse-click',button:'left',downMs:30,upMs:30}:kind==='scroll'?{kind:'wheel',horizontal:0,vertical:1}:{kind:'key-hold',key:44}}];
  for(const [index,definition]of actions.entries()){
    const contract=desktopActionContract(definition.action),nodeId=`${id}-transform-${index+1}`;
    if(kind==='hat-arrows')nodes.push({id:nodeId,name:`Hat ${['up','right','down','left'][index]} including diagonals`,operation:{kind:'hat-direction',direction:['up','right','down','left'][index],diagonals:true} as ProfileProcessingNode['operation'],inputs:[{id:'in',name:'Hat',direction:'input',contract:{delivery:'state',schema:{type:'integer',min:-1,max:7,unit:null},freshnessMs:1000,fallback:-1}}],outputs:[{id:'out',name:'Direction held',direction:'output',contract}]});
    else if(kind==='stick-pointer')nodes.push({id:nodeId,name:`Pointer ${index===0?'X':'Y'} dead zone`,operation:{kind:'deadzone',centerPercent:50,deadzonePercent:12,rounding:'none'},inputs:[{id:'in',name:'Normalized stick',direction:'input',contract}],outputs:[{id:'out',name:'Pointer velocity',direction:'output',contract}]});
    else if(kind==='encoder-tap'||kind==='mouse-click'||kind==='scroll')nodes.push({id:nodeId,name:'Press edge → ordered action',operation:{kind:'event-edge',edge:'rising'},inputs:[{id:'in',name:'Button',direction:'input',contract:{delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false}}],outputs:[{id:'out',name:'Press event',direction:'output',contract}]});
    if(nodes.some(node=>node.id===nodeId))routes.push({from:{nodeId,portId:'out'},to:{nodeId:id,portId:definition.portId}});
  }
  return{output:{id,name:'Keyboard and mouse',required:true,scope:{kind:'application'},ports:actions.map(definition=>desktopActionPort(definition.portId,desktopActionLabel(definition.action),definition.action)),actions},nodes,routes};
}
