import { firmwareUpdateAvailable } from './companion-firmware.ts';
import type { CompanionHostState, CompanionOperation } from './companions.ts';

export type CompanionMessage = { source:'connection'|'firmware'; kind:'info'|'warning'|'error'; message:string };
const text=(client:Record<string,unknown>,key:string)=>typeof client[key]==='string'?client[key] as string:'';

function operationMessage(value:unknown):CompanionMessage|undefined {
  if(!value||typeof value!=='object')return;
  const operation=value as CompanionOperation;
  const labels:Record<string,string>={opening:'Opening panel','checking-cache':'Checking cached images',downloading:'Preparing panel images',rendering:'Rendering panel','awaiting-neutral':'Waiting for the display and released touch'};
  if(operation.phase==='failed')return {source:'connection',kind:'error',message:`${operation.message||'The screen could not load the panel'}${operation.errorCode?` (${operation.errorCode})`:''}. ${operation.retryable?'Check the connection and retry.':'Check the panel and companion before retrying.'}`};
  if(!labels[operation.phase])return;
  let message=operation.phase==='downloading'?labels.downloading:operation.message||labels[operation.phase];
  const total=operation.totalBytes??0,completed=operation.completedBytes??0;
  if(total>0&&Number.isFinite(total)&&Number.isFinite(completed))message+=` · ${Math.min(100,Math.floor(completed/total*100))}% (${Math.ceil(completed/1024)} / ${Math.ceil(total/1024)} KiB)`;
  if((operation.cacheHits??0)+(operation.cacheMisses??0)>0)message+=` · ${operation.cacheHits??0} cached, ${operation.cacheMisses??0} ${['rendering','awaiting-neutral'].includes(operation.phase)?'downloaded':'to download'}`;
  return {source:'connection',kind:'info',message};
}

export function companionMessages(state:CompanionHostState,client:Record<string,unknown>):CompanionMessage[] {
  const messages:CompanionMessage[]=[];
  const id=text(client,'companionId'),panelError=text(client,'panelError'),connection=text(client,'connectionStatus');
  const usb=state.usb?.devices?.find(device=>device.device?.companionId===id)??(state.usb?.device?.companionId===id?state.usb:undefined);
  const assigned=(state.host?.instances??[]).filter(instance=>instance.companionId===id);
  const operation=client.operation??(usb?.connected&&usb.device?.operation&&assigned.some(instance=>instance.instanceId===usb.device?.operation?.instanceId&&instance.publicationHash===usb.device?.operation?.publicationHash)?usb.device.operation:undefined);
  const progress=operationMessage(operation);
  if(progress)messages.push(progress);
  else if(panelError)messages.push({source:'connection',kind:'error',message:`Panel error. ${panelError}`});
  else if(connection&&connection!=='No panel assigned')messages.push({source:'connection',kind:usb?.device?.status==='error'?'error':usb?.device?.status==='offline'?'warning':'info',message:connection});
  if(client.kind==='esp32'){
    const firmware=state.firmware?.[id];
    if(firmware&&firmwareUpdateAvailable(firmware.version,state.firmwareVersion))messages.push({source:'firmware',kind:'warning',message:'Firmware update available'});
    else if(!firmware)messages.push({source:'firmware',kind:'info',message:'Firmware version unknown · connect USB to check'});
  }
  return messages;
}
