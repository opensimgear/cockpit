export type DeviceKind = 'shifter' | 'pedals' | 'gateway' | 'wheel' | 'handbrake' | 'panel';
export type Settings = { debounce: number; inverted: boolean; label: string };
export type Device = {
  id: string;
  name: string;
  kind: DeviceKind;
  subtitle: string;
  online: boolean;
  route: string;
  inputs: string;
  firmware: string;
  settings: Settings;
};

export function settingsEqual(a: Settings, b: Settings): boolean {
  return a.debounce === b.debounce && a.inverted === b.inverted && a.label === b.label;
}
