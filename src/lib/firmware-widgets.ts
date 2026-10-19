import { applyDesignerCommand, createDesignerHistory, widgetPresets, type DesignerDocument } from '../../packages/companion-designer/src/index.ts';

const supported = new Set(['button', 'indicator', 'text', 'numeric', 'gauge', 'image']);
export const firmwareWidgetPresets = widgetPresets.filter(preset => supported.has(preset.kind));
/** Use editor commands so removed widgets also lose their controls and bindings. */
export function simplifyFirmwareWidgets(document: DesignerDocument) {
  let history = createDesignerHistory(document);
  const removed = document.presentation.widgets.filter(widget => !supported.has(widget.kind));
  for (const widget of removed) history = applyDesignerCommand(history, { type: 'delete-widget', id: widget.id });
  return { document: history.present, removed: removed.length };
}
/** Limits enforced by the current ESP32 decoder; browser panels are unrestricted. */
export function firmwarePanelReason(document:DesignerDocument):string {
  const unsupported=(detail:string)=>`ESP32 panel: ${detail}`;
  if(document.presentation.viewports.some(v=>v.backgroundAssetId) || document.presentation.widgets.some(w=>(w.config as Record<string,unknown>).iconAssetId))return unsupported('use image widgets instead of backgrounds or button icons.');
  if(document.presentation.groups.length || document.presentation.widgets.some(w=>w.groupId))return unsupported('groups are not supported. Ungroup widgets before saving.');
  if(document.ui.pages.length>4 || document.presentation.widgets.length>32)return unsupported('use at most 4 pages and 32 widgets.');
  if(new TextEncoder().encode(JSON.stringify(document)).length>16384)return unsupported('the panel exceeds the 16 KB device document limit.');
  const text=(s:unknown)=>typeof s==='string' && /^[A-Za-z0-9 .,:;\-_+=|/%!()?]*$/.test(s);
  if(document.ui.pages.some(p=>!text(p.title)) || document.presentation.widgets.some(w=>!text(w.label)||['prefix','suffix','unit'].some(key=>(w.config as Record<string,unknown>)[key]!==undefined&&!text((w.config as Record<string,unknown>)[key]))))return unsupported('labels and text must use letters, numbers and basic punctuation supported by the device font.');
  const imageBytes=document.presentation.widgets.filter(w=>w.kind==='image').reduce((sum,w)=>{
    const screen=document.presentation.viewports.find(v=>v.pageId===w.pageId);if(!screen)return sum;
    const m=screen.safeInsets;return sum+Math.floor(w.bounds.width*screen.width*(1-(m?.left??0)-(m?.right??0)))*Math.floor(w.bounds.height*screen.height*(1-(m?.top??0)-(m?.bottom??0)))*3;
  },0);
  if(imageBytes>1200*1024)return unsupported('image widgets exceed the 1200 KB device pixel budget. Reduce their size or number.');
  let strings=document.ui.id.length+73;
  for(const page of document.ui.pages)strings+=page.id.length+page.title.length+2;
  for(const w of document.presentation.widgets){
    const cfg=w.config as Record<string,unknown>;
    strings += [w.id,w.controlIds[0]??document.ui.feedbackWidgets.find(f=>f.id===w.feedbackIds[0])?.channelId??'',w.feedbackIds[0]??'',w.label,cfg.prefix??'',cfg.suffix??'',cfg.unit??''].reduce<number>((sum,v)=>sum+String(v).length+1,0);
    if(w.kind==='button'){
      const screen=document.presentation.viewports.find(v=>v.pageId===w.pageId);
      if(screen){const m=screen.safeInsets,scale=Math.min(1,screen.height/screen.height);
        if(Math.floor(w.bounds.width*(1-(m?.left??0)-(m?.right??0))*screen.width*scale)<44 || Math.floor(w.bounds.height*(1-(m?.top??0)-(m?.bottom??0))*screen.height*scale)<44)return unsupported('buttons must render at least 44 × 44 pixels on the device. Enlarge the button.');
      }
    }
    if(w.kind==='image' && !w.config.assetId)return unsupported('choose an asset for every image widget.');
    if(!supported.has(w.kind))return unsupported(`${w.kind} widgets are not supported.`);
    if(w.kind==='button'&&(w.config.buttonMode!=='momentary'||w.config.pulseMs!==0||w.config.repeatMs!==0))return unsupported('buttons must be momentary, without pulse or repeat.');
    if((w.kind==='numeric'||w.kind==='gauge') && (Number(w.config.precision)<0||Number(w.config.precision)>4||!Number.isInteger(w.config.precision)||Number(w.config.min)<-1e9||Number(w.config.max)>1e9))return unsupported('numeric precision must be 0–4 and ranges must stay within ±1,000,000,000.');
  }
  if(strings>8192)return unsupported('widget text exceeds the 8 KB device limit. Shorten labels or remove widgets.');
  return '';
}
