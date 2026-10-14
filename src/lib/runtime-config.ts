export type RuntimeConfig = { name: string; expanderMask: number; buttons: number[][]; axes: number[][]; hats: number[][]; encoders: number[][]; selectors: number[][] };
export type RuntimeSnapshot = { generation: number; savedGeneration: number; owner: number; claimExpiresMs: number; dirty: boolean; pendingApply: boolean; valid: boolean; view: 'draft' | 'active'; config: RuntimeConfig; layout: { buttons: number; axes: number; hats: number; reportBytes: number } | null; descriptorHex: string };
export type MappingKind = 'buttons' | 'axes' | 'hats' | 'encoders' | 'selectors';
export const mappingLimits: Record<MappingKind,number> = { buttons:128,axes:8,hats:4,encoders:8,selectors:8 };
export const mappingFields: Record<MappingKind,{ label:string; min:number; max:number; toggle?:boolean }[]> = {
  buttons: [{label:'Contact',min:0,max:151},{label:'Maintained',min:0,max:1,toggle:true},{label:'Exclusive group',min:0,max:255}],
  axes: [{label:'ADC channel',min:0,max:7},{label:'HID usage',min:48,max:56},{label:'Raw minimum',min:0,max:4094},{label:'Raw maximum',min:1,max:4095},{label:'Neutral',min:0,max:65535},{label:'Invert',min:0,max:1,toggle:true},{label:'Deadzone',min:0,max:32767}],
  hats: [{label:'Up',min:0,max:151},{label:'Right',min:0,max:151},{label:'Down',min:0,max:151},{label:'Left',min:0,max:151}],
  encoders: [{label:'Phase A',min:0,max:23},{label:'Phase B',min:0,max:23}],
  selectors: [{label:'Low contact',min:0,max:151},{label:'High contact',min:0,max:151}],
};
export function cloneConfig(config:RuntimeConfig):RuntimeConfig { return JSON.parse(JSON.stringify(config)); }
export function configEqual(a:RuntimeConfig,b:RuntimeConfig):boolean { return JSON.stringify(a)===JSON.stringify(b); }
export function configurationError(config:RuntimeConfig):string {
  if (!config.name.trim() || new TextEncoder().encode(config.name).length>63 || /[\u0000-\u001f\u007f]/.test(config.name)) return 'Choose a device name of 1–63 UTF-8 bytes without control characters.';
  const logical=config.buttons.length+2*config.encoders.length+3*config.selectors.length;
  if (logical>128) return 'Buttons, encoders and selectors together may expose at most 128 buttons.';
  const used=new Set<number>();
  for (const kind of Object.keys(mappingFields) as MappingKind[]) {
    if (config[kind].length>mappingLimits[kind]) return `Too many ${kind}.`;
    for (const [index,row] of config[kind].entries()) {
      const fields=mappingFields[kind];
      if (row.length!==fields.length || row.some((value,column)=>!Number.isInteger(value)||value<fields[column].min||value>fields[column].max)) return `Check ${kind} ${index+1}: values must be whole numbers within the supported ranges.`;
      if(kind==='axes') { if(row[2]>=row[3]) return `Axis ${index+1}: raw minimum must be below maximum.`; continue; }
      if(kind==='buttons'&&row[1]&&row[2]) return `Button ${index+1}: maintained switches cannot belong to an exclusive group.`;
      const contacts=kind==='buttons' ? [row[0]] : row;
      for(const input of contacts) {
        if(input>=24 && !(config.expanderMask & (1 << Math.floor((input-24)/16)))) return `Contact ${input} needs expansion bank ${Math.floor((input-24)/16)+1}. Enable that bank below.`;
        if(used.has(input)) return `Contact ${input} is assigned to more than one control. Use a separate contact for each input.`;
        used.add(input);
      }
    }
  }
  if(new Set(config.axes.map(axis=>axis[0])).size!==config.axes.length) return 'Each axis needs a unique ADC channel.';
  if(new Set(config.axes.map(axis=>axis[1])).size!==config.axes.length) return 'Each axis needs a unique HID usage.';
  return '';
}
export function resizeMappings(config:RuntimeConfig,kind:MappingKind,count:number):void {
  if(!Number.isInteger(count)||count<0||count>mappingLimits[kind]) return;
  config[kind]=config[kind].slice(0,count);
  const used=new Set((Object.keys(mappingFields) as MappingKind[]).filter(key=>key!=='axes').flatMap(key=>config[key].flatMap(row=>key==='buttons'?[row[0]]:row)));
  const contact=()=>{let value=0;while(used.has(value))value++;used.add(value);return value;};
  while(config[kind].length<count) {
    const index=config[kind].length;
    const channels=new Set(config.axes.map(row=>row[0]));
    const usages=new Set(config.axes.map(row=>row[1]));
    let channel=0,usage=48; while(channels.has(channel))channel++;while(usages.has(usage))usage++;
    config[kind].push(kind==='axes' ? [channel,usage,0,4095,0,0,0] : kind==='buttons' ? [contact(),0,0] : Array.from({length:mappingFields[kind].length},contact));
  }
}
