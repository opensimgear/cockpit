import type { WidgetDefinition, WidgetLayer } from './widget-model.ts';

/** Editable, asset-free capsule switch. OFF is left/bottom; confirmed ON is right/top. */
export function createCapsuleToggle(vertical: boolean): WidgetDefinition {
  const width = vertical ? 104 : 200, height = vertical ? 200 : 128;
  const bounds = (x: number, y: number, w: number, h: number) => ({ x: x / width, y: y / height, width: w / width, height: h / height });
  const track = vertical ? bounds(24, 52, 56, 128) : bounds(36, 56, 128, 56);
  const layers: WidgetLayer[] = [
    { id: 'title', label: 'Title', kind: 'text', bounds: bounds(0, 3, width, 34), text: { property: 'label' }, font: 'sans', fontSize: 24, align: 'center', fill: { property: 'foreground' }, states: { disabled: { opacity: .4 }, disconnected: { opacity: .4 }, stale: { opacity: .6 } } },
    { id: 'track-glow', label: 'Track halo', kind: 'rectangle', bounds: track, radius: 28, stroke: { property: 'edge' }, strokeWidth: 6, opacity: .12, states: { pressed: { opacity: .3 }, disabled: { opacity: .03 }, disconnected: { opacity: .03 } } },
    { id: 'track', label: 'Capsule track', kind: 'rectangle', bounds: track, radius: 28, fill: { property: 'background' }, stroke: { property: 'edge' }, strokeWidth: 2, states: { active: { stroke: { property: 'accent' } }, pressed: { stroke: { property: 'accent' }, strokeWidth: 3 }, warning: { stroke: '#efbc73' }, disabled: { opacity: .4 }, disconnected: { opacity: .4 }, stale: { opacity: .6 } } },
  ];
  for (const on of [false, true]) {
    const x = vertical ? 52 : on ? 136 : 64;
    const y = vertical ? on ? 80 : 152 : 84;
    const side = on ? 'on' : 'off';
    for (const [part, diameter, opacity] of [['halo', 62, .035], ['glow', 52, .12], ['thumb', 44, .9], ['highlight', 34, .2]] as const) {
      layers.push({ id: `${side}-${part}`, label: `${on ? 'On' : 'Off'} ${part}`, kind: 'ellipse', bounds: bounds(x - diameter / 2, y - diameter / 2, diameter, diameter), fill: { property: part === 'highlight' ? 'highlight' : 'accent' }, opacity, visible: !on, states: { normal: { visible: !on }, active: { visible: on }, pressed: { visible: !on, opacity: opacity * .7 }, warning: { visible: !on, opacity: opacity * .6 }, stale: { visible: !on, opacity: opacity * .4 }, unknown: { visible: !on, opacity: opacity * .4 }, disabled: { visible: !on, opacity: opacity * .4 }, disconnected: { visible: !on, opacity: opacity * .4 } } });
    }
  }
  return {
    formatVersion: 1, id: `osg-capsule-toggle-${vertical ? 'vertical' : 'horizontal'}`, version: 2,
    title: `Capsule toggle · ${vertical ? 'Vertical' : 'Horizontal'}`, size: { width, height },
    properties: [
      { name: 'label', label: 'Title', type: 'text', default: vertical ? 'PWR' : 'GSAFE' },
      { name: 'accent', label: 'Thumb glow', type: 'color', default: '#bde7ec' },
      { name: 'highlight', label: 'Thumb center', type: 'color', default: '#d7f1f4' },
      { name: 'foreground', label: 'Title color', type: 'color', default: '#d2edf0' },
      { name: 'background', label: 'Track color', type: 'color', default: '#252d30' },
      { name: 'edge', label: 'Outline', type: 'color', default: '#a9c6cb' },
    ],
    inputs: [{ name: 'press', label: 'Toggle', kind: 'button' }], outputs: [{ name: 'active', label: 'Confirmed on', kind: 'indicator' }],
    interaction: { kind: 'toggle', port: 'press' }, state: { confirmedPort: 'active' },
    hitArea: { x: 0, y: 0, width: 1, height: 1 }, layers,
  };
}
