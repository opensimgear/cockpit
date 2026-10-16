import type { CompanionHostState } from './companions';

export function selectedUsbIsPaired(state: CompanionHostState, port: string): boolean {
  const usb=state.usb;
  return !!port && usb?.port===port && !!usb.device?.companionId
    && (state.host?.clients??[]).some(c=>c.companionId===usb.device!.companionId);
}

export function newPairingConnected(kind: string, baseline: Set<string>, state: CompanionHostState): boolean {
  if(!kind)return false;
  return (state.host?.clients??[]).some(client=>{
    const id=typeof client.companionId==='string'?client.companionId:'';
    if(!id || baseline.has(id) || client.connected!==true)return false;
    if(kind==='esp32')return state.usb?.device?.setupSaved===true && state.usb.device.companionId===id;
    return client.kind===kind;
  });
}
