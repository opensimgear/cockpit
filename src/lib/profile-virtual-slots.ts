import type {DeepReadonly, ProfileControllerLayout} from './profiles';

export type VirtualSlotAssignment = {nodeId:string;slot:number|null};
export type VirtualSlotAssignments = {profileId:string;profileRevision:number;readOnly:boolean;virtualOutputIds:string[];virtualSlots:VirtualSlotAssignment[]};
export type VirtualBackendSlot = {slot:number;name:string;status:string;buttons:number|null;axes:{usage:number;min:number;max:number}[]|null;hats:number|null;discreteHats:number|null;forceFeedback:boolean|null};
const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
const integer=(value:unknown,min:number,max:number):value is number=>Number.isInteger(value)&&Number(value)>=min&&Number(value)<=max;
const slotNumber=(value:unknown):value is number=>integer(value,1,16);
const usages=[0x30,0x31,0x32,0x33,0x34,0x35,0x36,0x37,0x38,0xc4,0xc5,0xc6,0xc8,0xb0,0xba,0xbb];

export function virtualSlotAssignments(value:unknown,profileId:string):VirtualSlotAssignments {
  const ids=object(value)&&Array.isArray(value.virtualOutputIds)?value.virtualOutputIds:[];
  if(!object(value)||value.profileId!==profileId||!integer(value.profileRevision,1,Number.MAX_SAFE_INTEGER)||typeof value.readOnly!=='boolean'
    ||!Array.isArray(value.virtualOutputIds)||value.virtualOutputIds.length>16||!value.virtualOutputIds.every(id=>typeof id==='string')
    ||!Array.isArray(value.virtualSlots)||value.virtualSlots.length>16||!value.virtualSlots.every(pin=>object(pin)&&typeof pin.nodeId==='string'&&ids.includes(pin.nodeId)&&(pin.slot===null||slotNumber(pin.slot)))
    ||new Set(value.virtualSlots.map(pin=>pin.nodeId)).size!==value.virtualSlots.length)throw Error('Virtual slot settings are unavailable. Refresh and try again.');
  return structuredClone(value) as VirtualSlotAssignments;
}

/** Observational capabilities only; Apply checks fresh inventory and ownership. */
export function virtualBackendSlots(value:unknown):VirtualBackendSlot[] {
  if(!object(value)||!Array.isArray(value.virtualDevices))throw Error('Virtual controller inventory is unavailable.');
  const result:VirtualBackendSlot[]=[];
  for(const row of value.virtualDevices){
    if(!object(row)||!slotNumber(row.slot)||result.some(slot=>slot.slot===row.slot))continue;
    const axes=Array.isArray(row.axes)&&row.axes.length<=16&&row.axes.every(axis=>object(axis)&&integer(axis.usage,0,65535)&&integer(axis.min,-2147483648,2147483647)&&integer(axis.max,-2147483648,2147483647))?row.axes as VirtualBackendSlot['axes']:null;
    result.push({slot:row.slot,name:typeof row.name==='string'?row.name:`Slot ${row.slot}`,status:typeof row.status==='string'?row.status:'unknown',
      buttons:integer(row.buttons,0,128)?row.buttons:null,axes,hats:integer(row.continuousHats,0,4)?row.continuousHats:null,
      discreteHats:integer(row.discreteHats,0,4)?row.discreteHats:null,forceFeedback:typeof row.forceFeedback==='boolean'?row.forceFeedback:null});
  }
  return result.sort((a,b)=>a.slot-b.slot);
}

export function virtualSlotSummary(slot:VirtualBackendSlot):string {
  if(slot.buttons===null||slot.axes===null||slot.hats===null)return 'Layout unavailable';
  return `${slot.buttons} buttons · ${slot.axes.length} axes · ${slot.hats} hats`;
}

export function virtualSlotLayoutIssue(slot:VirtualBackendSlot,layout:DeepReadonly<ProfileControllerLayout>):string {
  if(slot.buttons===null||slot.axes===null||slot.hats===null||slot.discreteHats===null||slot.forceFeedback===null)return 'The slot’s layout is unavailable. Refresh its inventory before activating.';
  if(slot.forceFeedback||slot.discreteHats||slot.axes.some(axis=>axis.min!==0||axis.max!==32767||!usages.includes(axis.usage))||new Set(slot.axes.map(axis=>axis.usage)).size!==slot.axes.length)return 'This slot has unsupported axis, force feedback or discrete hat settings.';
  if(slot.buttons!==layout.buttons.length||slot.axes.length!==layout.axes.length||slot.hats!==layout.hats.length)return `Slot ${slot.slot} has ${virtualSlotSummary(slot).toLowerCase()}. Match its layout before activating.`;
  return '';
}

/** Existing logical IDs and axis metadata survive; graph validation protects connected inputs. */
export function virtualLayoutForSlot(slot:VirtualBackendSlot,current:DeepReadonly<ProfileControllerLayout>):ProfileControllerLayout {
  if(slot.buttons===null||slot.axes===null||slot.hats===null)throw Error('The slot’s layout is unavailable.');
  const counts={buttons:slot.buttons,axes:slot.axes.length,hats:slot.hats};
  if(counts.buttons+counts.axes+counts.hats<1||counts.buttons+counts.axes+counts.hats>128)throw Error('Choose a slot with 1–128 total controls.');
  const layout:ProfileControllerLayout=JSON.parse(JSON.stringify(current));
  const used=new Set([...layout.buttons,...layout.axes.map(axis=>axis.portId),...layout.hats]);
  const fresh=(prefix:string)=>{let index=1;while(used.has(`${prefix}-${index}`))index++;const id=`${prefix}-${index}`;used.add(id);return id;};
  layout.buttons=Array.from({length:counts.buttons},(_,index)=>layout.buttons[index]??fresh('button'));
  layout.axes=Array.from({length:counts.axes},(_,index)=>layout.axes[index]??{portId:fresh('axis'),range:'bipolar',fallback:0});
  layout.hats=Array.from({length:counts.hats},(_,index)=>layout.hats[index]??fresh('hat'));
  const error=virtualSlotLayoutIssue(slot,layout);if(error)throw Error(error);
  return layout;
}
