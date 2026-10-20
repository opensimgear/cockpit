import { firmwareUpdateAvailable } from './companion-firmware.ts';
import type { CompanionHostState } from './companions.ts';

export type CompanionMessage = { source:'connection'|'firmware'; kind:'info'|'warning'|'error'; message:string };
const text=(client:Record<string,unknown>,key:string)=>typeof client[key]==='string'?client[key] as string:'';

export function companionMessages(state:CompanionHostState,client:Record<string,unknown>):CompanionMessage[] {
  const messages:CompanionMessage[]=[];
  const id=text(client,'companionId'),panelError=text(client,'panelError'),connection=text(client,'connectionStatus');
  const usb=state.usb?.devices?.find(device=>device.device?.companionId===id)??(state.usb?.device?.companionId===id?state.usb:undefined);
  if(panelError)messages.push({source:'connection',kind:'error',message:`Panel error. ${panelError}`});
  else if(connection&&connection!=='No panel assigned')messages.push({source:'connection',kind:usb?.device?.status==='error'?'error':usb?.device?.status==='offline'?'warning':'info',message:connection});
  if(client.kind==='esp32'){
    const firmware=state.firmware?.[id];
    if(firmware&&firmwareUpdateAvailable(firmware.version,state.firmwareVersion))messages.push({source:'firmware',kind:'warning',message:'Firmware update available'});
    else if(!firmware)messages.push({source:'firmware',kind:'info',message:'Firmware version unknown · connect USB to check'});
  }
  return messages;
}
