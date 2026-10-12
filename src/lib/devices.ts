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

// Illustrative inventory. No serial discovery or firmware compatibility claims.
export const demoDevices: Device[] = [
  { id: 'OSG-SQ-0042', name: 'OSG Sequential shifter', kind: 'shifter', subtitle: 'Shift with intention.', online: true, route: 'Gateway 01 · CAN', inputs: '3 digital inputs', firmware: 'Demo 0.1.0', settings: { debounce: 10, inverted: false, label: 'OSG Sequential shifter' } },
  { id: 'OSG-PD-0018', name: 'OSG Pedal set', kind: 'pedals', subtitle: 'Every input, precisely.', online: true, route: 'Gateway 01 · CAN', inputs: '3 axes', firmware: 'Demo 0.1.0', settings: { debounce: 10, inverted: false, label: 'OSG Pedal set' } },
  { id: 'OSG-BP-0007', name: 'OSG Button panel', kind: 'panel', subtitle: 'Your controls, within reach.', online: true, route: 'Gateway 01 · CAN', inputs: '12 buttons · 2 encoders', firmware: 'Demo 0.1.0', settings: { debounce: 10, inverted: false, label: 'OSG Button panel' } },
  { id: 'OSG-HB-0023', name: 'OSG Handbrake', kind: 'handbrake', subtitle: 'Ready when you are.', online: false, route: 'Direct USB · last seen', inputs: '1 axis', firmware: 'Demo 0.1.0', settings: { debounce: 10, inverted: false, label: 'OSG Handbrake' } },
];

export function settingsEqual(a: Settings, b: Settings): boolean {
  return a.debounce === b.debounce && a.inverted === b.inverted && a.label === b.label;
}
