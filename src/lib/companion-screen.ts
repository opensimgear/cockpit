import type { Publication } from '../../packages/companion-designer/src/index.ts';
import type { CompanionHostState } from './companions.ts';

export type ScreenSize = { width:number; height:number };
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
export function panelScreenReason(panel:Publication,screen?:ScreenSize):string {
  if(!screen)return 'Connect this ESP32 screen by USB to identify its screen size before assigning a panel.';
  if(panel.document.ui.pages.some(page=>!panel.document.presentation.viewports.some(viewport=>viewport.pageId===page.id&&viewport.width===screen.width&&viewport.height===screen.height)))return `Every panel page must be ${screen.width} × ${screen.height} px to match this screen.`;
  return '';
}
