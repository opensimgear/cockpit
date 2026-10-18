/** Local desktop preferences and native observation; no live effects cross IPC. */
export type DesktopScope={kind:'application'}|{kind:'desktop-wide'};
export type DesktopStatus={enabled:boolean;qualified:boolean;state:string;reason:string|null;scope:DesktopScope;application?:unknown;error?:string|null;runtime?:unknown};
export type DesktopCommand='profile_desktop_status'|'profile_desktop_configure'|'profile_desktop_capture_application'|'profile_desktop_stop'|'profile_desktop_recover';
export type DesktopRequest={command:'profile_desktop_configure';enabled:boolean;scope:DesktopScope}|{command:Exclude<DesktopCommand,'profile_desktop_configure'>};
export function desktopStatus(value:unknown):DesktopStatus {
  if(!value||typeof value!=='object')throw new Error('Native desktop input status is unavailable.');
  const status=value as Record<string,unknown>,scope=status.scope as Record<string,unknown>|undefined;
  if(typeof status.enabled!=='boolean'||typeof status.qualified!=='boolean'||typeof status.state!=='string'||!(status.reason===null||typeof status.reason==='string')||!scope||!['application','desktop-wide'].includes(String(scope.kind)))throw new Error('Native desktop input status is malformed. Refresh status.');
  return JSON.parse(JSON.stringify(value)) as DesktopStatus;
}
