import type { HidHideSelfAccess, VisibilityCommand, VisibilityStatus } from './visibility';
export type ControllerCommand = VisibilityCommand | 'controller_list' | 'controller_inspect' | 'controller_update' | 'controller_forget' | 'controller_reassign';
export type HostController = {
  id: string; name: string; role: string; origin: 'host-controller'; backend: string;
  vendorId: number; productId: number; osgHid: boolean; online: boolean;
  generation: string; sequence: string; controlCount: number; baselineReady: boolean;
  identityConfidence: 'serial' | 'local-path';
};
export type ControllerControl = {
  capability: { id: string; kind: 'button' | 'axis' | 'hat' | 'diagnostic'; usagePage: number; usage: number;
    reportId: number; collection: number; dataIndex: number; bits: number; min: number; max: number; hasNull: boolean };
  value: { native: number; normalized: number | null } | null;
};
export type ControllerResult = {
  visibility?: VisibilityStatus;
  selfAccess?: HidHideSelfAccess;
  session?: string; backendStatus?: string; diagnostic?: string; sources?: HostController[];
  source?: HostController; identity?: { path: string; collection: string; serial: string | null; fingerprint: string };
  diagnostics?: string[]; controls?: ControllerControl[]; offset: number; total: number; limit: number;
};
export const controllerRoles = ['controller', 'joystick', 'wheelbase', 'pedals', 'throttle', 'panel'];
export function controlLabel(control: ControllerControl): string {
  const c = control.capability;
  if (c.kind === 'button') return `Button ${c.usage}`;
  if (c.kind === 'hat') return 'Hat';
  const axes: Record<number, string> = { 48: 'X', 49: 'Y', 50: 'Z', 51: 'Rx', 52: 'Ry', 53: 'Rz', 54: 'Slider', 55: 'Dial', 56: 'Wheel' };
  return c.usagePage === 1 && axes[c.usage] ? axes[c.usage] : `Usage ${c.usagePage.toString(16)}:${c.usage.toString(16)}`;
}
export function inputLabel(control: ControllerControl): string {
  if (!control.value) return 'Waiting for report';
  if (control.capability.kind === 'diagnostic') return `Raw ${control.value.native}`;
  if (control.capability.kind === 'button') return control.value.normalized ? 'Pressed' : 'Released';
  if (control.capability.kind === 'hat') return control.value.normalized === null ? 'Neutral' : ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'][control.value.normalized] ?? 'Unknown';
  return String(control.value.normalized);
}
