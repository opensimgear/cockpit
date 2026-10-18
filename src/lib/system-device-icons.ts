import type { SystemComponent } from './system-inventory';

/** Visual hints only: classification never changes device capabilities or routing. */
export function systemDeviceIcon(component: Pick<SystemComponent, 'type' | 'role' | 'name'>): string {
  if (component.type === 'companion') return 'devices';
  if (component.type === 'host') return 'monitor';
  if (component.type === 'gateway') return 'network';
  if (component.type === 'integration') return 'link';
  if (component.type === 'virtual') return 'gamepad';
  const roles: Record<string, string> = { controller: 'gamepad', joystick: 'joystick', wheelbase: 'wheel', wheel: 'wheel', pedals: 'pedals', throttle: 'throttle', panel: 'panel' };
  if (component.role !== 'controller' && roles[component.role]) return roles[component.role];
  const name = component.name.toLowerCase();
  if (/pedal|rudder/.test(name)) return 'pedals';
  if (/joystick|gladiator|flight stick|flightstick/.test(name)) return 'joystick';
  if (/wheel|moza.*base/.test(name)) return 'wheel';
  if (/throttle/.test(name)) return 'throttle';
  return roles[component.role] ?? (component.type === 'osg' ? 'chip' : 'gamepad');
}
