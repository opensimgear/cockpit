import {DESKTOP_KEYS, desktopActionPort, desktopKeyLabel, type DesktopAction, type ProfileDesktopOutput} from './profile-desktop.ts';
import type {DeepReadonly} from './profiles';

export type DesktopBlockKind = 'keyboard' | 'mouse';
export function desktopDevices(output:DeepReadonly<Pick<ProfileDesktopOutput,'actions'>>):DesktopBlockKind[] {
  return [...new Set(output.actions.map(({action}) => action.kind.startsWith('mouse-') || action.kind === 'wheel' || action.kind === 'pointer' ? 'mouse' as const : 'keyboard' as const))];
}
export function desktopBlockLabel(output:DeepReadonly<Pick<ProfileDesktopOutput,'actions'>>):string {
  const devices = desktopDevices(output);
  return devices.length > 1 ? 'Keyboard and mouse' : devices[0] === 'mouse' ? 'Mouse' : 'Keyboard';
}
/** Both blocks share the existing native desktop adapter and its output scope. */
export function desktopOutputsValid(outputs:readonly DeepReadonly<ProfileDesktopOutput>[]):boolean {
  const devices = outputs.flatMap(desktopDevices);
  const scope = (output:DeepReadonly<ProfileDesktopOutput>) => output.scope?.kind === 'application' ? `application:${output.scope.applicationReference??''}` : output.scope?.kind??'';
  return outputs.length <= 2 && new Set(devices).size === devices.length && outputs.every(output => scope(output) === scope(outputs[0]));
}
export function desktopInputLabel(action:DeepReadonly<DesktopAction>):string {
  if ('key' in action) return desktopKeyLabel(action.key);
  if (action.kind === 'chord') return action.keys.map(desktopKeyLabel).join(' + ');
  if ('button' in action) return ({left:'Left',right:'Right',middle:'Middle',x1:'Back',x2:'Forward'})[action.button];
  if (action.kind === 'pointer') return `Pointer ${action.axis.toUpperCase()}`;
  return action.vertical > 0 ? 'Scroll up' : action.vertical < 0 ? 'Scroll down' : action.horizontal > 0 ? 'Scroll right' : 'Scroll left';
}
/** Input labels describe the key; connection transforms determine its behavior. */
export function desktopDisplayPorts(output:DeepReadonly<ProfileDesktopOutput>) {
  return output.ports.map(port => {
    const action=output.actions.find(definition=>definition.portId===port.id)?.action;
    return action&&'key' in action ? {...port,name:desktopKeyLabel(action.key)} : port;
  });
}
export function desktopKeycap(action:DeepReadonly<DesktopAction>):string {
  const label=desktopInputLabel(action);
  if (!('key' in action)) return label;
  const short:Record<string,string>={Escape:'Esc',Backspace:'Back',CapsLock:'Caps',PrintScreen:'PrtSc',ScrollLock:'ScrLk',PageUp:'PgUp',PageDown:'PgDn',NumLock:'Num',ContextMenu:'Menu',
    BracketLeft:'[',BracketRight:']',Backslash:'\\',Minus:'−',Equal:'=',Semicolon:';',Quote:"'",Backquote:'`',Comma:',',Period:'.',Slash:'/',
    'Right arrow':'→','Left arrow':'←','Down arrow':'↓','Up arrow':'↑','Non-US #':'#','Non-US backslash':'Intl \\'};
  return short[label]??label.replace(/^Numpad/,'N ').replace('Divide','/').replace('Multiply','×').replace('Subtract','−').replace('Add','+').replace('Decimal','.').replace('Enter','Ent').replace(/^Left /,'L ').replace(/^Right /,'R ');
}
/** A complete device catalogue is available before any route is authored. */
export function desktopBlock(kind:DesktopBlockKind,id:string):ProfileDesktopOutput {
  const actions:ProfileDesktopOutput['actions'] = kind === 'keyboard'
    ? DESKTOP_KEYS.map(({usage}) => ({portId:`key-${usage}`,action:{kind:'key-hold',key:usage}}))
    : [
      ...(['left','right','middle','x1','x2'] as const).map(button => ({portId:`button-${button}`,action:{kind:'mouse-hold' as const,button}})),
      ...(['x','y'] as const).map(axis => ({portId:`pointer-${axis}`,action:{kind:'pointer' as const,axis,speed:900,curve:1}})),
      ...([{id:'up',horizontal:0,vertical:1},{id:'down',horizontal:0,vertical:-1},{id:'left',horizontal:-1,vertical:0},{id:'right',horizontal:1,vertical:0}]).map(({id,horizontal,vertical}) => ({portId:`scroll-${id}`,action:{kind:'wheel' as const,horizontal,vertical}})),
    ];
  return {id,name:kind === 'keyboard' ? 'Keyboard' : 'Mouse',required:false,scope:{kind:'selected-game'},
    ports:actions.map(({portId,action}) => desktopActionPort(portId,desktopInputLabel(action),action)),actions};
}
