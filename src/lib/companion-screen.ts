import { firmwarePanelReason } from './firmware-widgets.ts';
import type { Publication } from '../../packages/companion-designer/src/index.ts';
import type { CompanionHostState } from './companions.ts';

export type ScreenSize = { width:number; height:number };
export type PanelScreenChoice = ScreenSize & { id:string; name:string };
export const PANEL_SCREEN_MIN = 320;
export const PANEL_SCREEN_MAX = 8192;

/** Matches the viewport dimensions accepted by the panel document model. */
export function panelScreenSizeReason(width:number,height:number):string {
  return [width,height].every(value=>Number.isInteger(value)&&value>=PANEL_SCREEN_MIN&&value<=PANEL_SCREEN_MAX)
    ? '' : `Screen width and height must be whole numbers between ${PANEL_SCREEN_MIN} and ${PANEL_SCREEN_MAX} pixels.`;
}

/** Screen dimensions are independent of connection state and panel assignment. */
export function knownPanelScreenChoices(state:CompanionHostState):PanelScreenChoice[] {
  const choices:PanelScreenChoice[]=[];
  const seen=new Set<string>();
  for(const client of state.host?.clients??[]) {
    const id=client.companionId;
    if(typeof id!=='string'||!id||seen.has(id))continue;
    const reported=client.screen;
    const screen=reported&&typeof reported==='object'&&'width' in reported&&'height' in reported
      &&typeof reported.width==='number'&&typeof reported.height==='number'&&!panelScreenSizeReason(reported.width,reported.height)
      ? {width:reported.width,height:reported.height} : companionScreenSize(state,id);
    if(!screen||panelScreenSizeReason(screen.width,screen.height))continue;
    const name=typeof client.name==='string'&&client.name.trim()?client.name.trim():id;
    choices.push({id,name,...screen});
    seen.add(id);
  }
  return choices;
}

export function boardScreenSize(profile?:string):ScreenSize|undefined {
  if(profile==='sunton-3248s035r')return {width:480,height:320};
  if(profile==='waveshare-lcd7-s3')return {width:800,height:480};
}
export function companionScreenSize(state:CompanionHostState,id:string):ScreenSize|undefined {
  const client=state.host?.clients?.find(client=>client.companionId===id);
  if(client?.kind!=='esp32')return;
  const screen=client.screen as ScreenSize|undefined;
  if(screen&&Number.isInteger(screen.width)&&Number.isInteger(screen.height)&&screen.width>0&&screen.height>0)return screen;
  const usb=state.usb?.devices?.find(usb=>usb.device?.companionId===id)??(state.usb?.device?.companionId===id?state.usb:undefined);
  return boardScreenSize(usb?.device?.boardProfile)??boardScreenSize(state.firmware?.[id]?.profile);
}
export function firmwareSupportsImages(version:string):boolean {
  const semantic=/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/.exec(version);
  if(semantic)return Number(semantic[1])>0 || Number(semantic[2])>=1;
  return /^\d{4}\.\d+\.\d+\.\d+$/.test(version) && version.localeCompare('2026.10.19.7',undefined,{numeric:true})>=0;
}
export function panelScreenReason(panel:Publication,screen?:ScreenSize,firmwareVersion?:string):string {
  if(firmwareVersion!==undefined && panel.document.presentation.widgets.some(w=>w.kind==='image') && !firmwareSupportsImages(firmwareVersion))return 'Update this screen to firmware 2026.10.19.7 or newer to display image widgets.';
  const unsupported=firmwarePanelReason(panel.document);if(unsupported)return unsupported;
  if(!screen)return 'Connect this ESP32 screen by USB to identify its screen size before assigning a panel.';
  if(panel.document.ui.pages.some(page=>!panel.document.presentation.viewports.some(viewport=>viewport.pageId===page.id&&viewport.width===screen.width&&viewport.height===screen.height)))return `Every panel page must be ${screen.width} × ${screen.height} px to match this screen.`;
  return '';
}
