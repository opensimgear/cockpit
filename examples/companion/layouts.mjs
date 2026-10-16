/** Reproducible public panel bundles; no host, credentials or output activation. */
import { writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { checked, normalizeToPortableUi, exportBundle } from '../../packages/companion-designer/src/index.ts';

function panel(id, title, pageTitles, axes) {
  const document = {
    schemaVersion: 2,
    ui: { schemaVersion: 1, id, title, controls: [], feedbackWidgets: [], assets: [], pages: pageTitles.map((title, index) => ({ id: `${id}-page-${index + 1}`, title, items: [] })) },
    presentation: { widgets: [], groups: [], grid: { columns: 24, rows: 16, snap: true }, viewports: pageTitles.map((_, index) => ({ pageId: `${id}-page-${index + 1}`, width: 390, height: 844, safeInsets: { top: 0.04, right: 0.02, bottom: 0.04, left: 0.02 } })) },
    targets: [{ id: `${id}-controller`, label: title, axes, buttons: 0, hats: 0 }], bindings: [],
  };
  const style = { foreground: '#F2F5F7', background: '#14232D', accent: '#56D4BD' };
  function add(kind, key, label, page, bounds, controls = [], config = {}, feedback) {
    const controlIds = [];
    for (const control of controls) {
      const id = `${document.ui.id}-${control.key}`;
      const { key: _, index, ...definition } = control;
      document.ui.controls.push({ id, ...definition }); controlIds.push(id);
      const target = document.targets[0];
      document.bindings.push({ controlId: id, targetId: target.id, kind: control.kind, index });
      if (control.kind === 'button') target.buttons = Math.max(target.buttons, index + 1);
      if (control.kind === 'hat') target.hats = Math.max(target.hats, index + 1);
    }
    const feedbackIds = [];
    if (feedback) {
      const id = `${document.ui.id}-${key}-output`;
      document.ui.feedbackWidgets.push({ id, label, kind: feedback.kind, channelId: `${document.ui.id}-${feedback.channel}` }); feedbackIds.push(id);
    }
    document.presentation.widgets.push({ id: `${document.ui.id}-${key}-widget`, pageId: document.ui.pages[page].id, kind, label, bounds, controlIds, feedbackIds, style: { ...style }, config });
  }
  return { document, add, finish() { document.ui = normalizeToPortableUi(document); return checked(document); } };
}
const button = (key, label, index) => ({ key, label, kind: 'button', index });
const axis = (key, label, index, axisMode) => ({ key, label, kind: 'axis', index, axisMode, fallback: 0 });
const buttonConfig = (buttonMode = 'momentary') => ({ buttonMode, pulseMs: buttonMode === 'pulse' ? 100 : 0, repeatMs: 0 });
const sliderConfig = orientation => ({ centering: 'spring', deadzone: 0.03, inverted: false, orientation });

export function flightButtonBox() {
  const p = panel('example-flight', 'Flight button box', ['Flight controls', 'Flight feedback'], ['unipolar', 'bipolar']);
  const cell = index => ({ x: 0.04 + (index % 3) * 0.32, y: 0.06 + Math.floor(index / 3) * 0.30, width: 0.28, height: 0.24 });
  p.add('button', 'transmit', 'Transmit (held)', 0, cell(0), [button('transmit', 'Transmit', 0)], buttonConfig());
  p.add('button', 'gear', 'Gear request (toggle)', 0, cell(1), [button('gear', 'Gear request', 1)], buttonConfig('toggle'));
  p.add('button', 'ack', 'Acknowledge (100 ms)', 0, cell(2), [button('ack', 'Acknowledge', 2)], buttonConfig('pulse'));
  p.add('button', 'lights', 'Lights request (toggle)', 0, cell(3), [button('lights', 'Lights request', 3)], buttonConfig('toggle'));
  p.add('encoder', 'radio', 'Radio preset - / +', 0, cell(4), [button('radio-up', 'Radio preset up', 4), button('radio-down', 'Radio preset down', 5)], { pulseMs: 50, repeatMs: 0 });
  p.add('hat', 'view', 'View hat', 0, cell(5), [{ key: 'view', label: 'View direction', kind: 'hat', index: 0, fallback: -1 }]);
  p.add('selector', 'flaps', 'Flaps request', 0, cell(6), [axis('flaps', 'Flaps request', 0, 'unipolar')], { options: [{ label: 'Up', value: 0 }, { label: 'Half', value: 0.5 }, { label: 'Full', value: 1 }] });
  p.add('button', 'aux', 'Auxiliary (held)', 0, cell(7), [button('aux', 'Auxiliary', 6)], buttonConfig());
  p.add('slider', 'trim', 'Trim (spring return)', 0, cell(8), [axis('trim', 'Trim', 1, 'bipolar')], sliderConfig('horizontal'));
  const left = { x: 0.06, y: 0.08, width: 0.42, height: 0.34 }, right = { ...left, x: 0.52 };
  p.add('indicator', 'gear-state', 'Gear requested', 1, left, [], {}, { kind: 'indicator', channel: 'gear-requested' });
  p.add('text', 'radio-text', 'Radio frequency', 1, right, [], { prefix: '', suffix: '' }, { kind: 'text', channel: 'radio-text' });
  p.add('numeric', 'trim-state', 'Trim requested', 1, { ...left, y: 0.52 }, [], { min: -1, max: 1, unit: 'ratio', precision: 2 }, { kind: 'gauge', channel: 'trim-requested' });
  p.add('gauge', 'rpm', 'Engine RPM', 1, { ...right, y: 0.52 }, [], { min: 0, max: 100000, unit: 'rpm', precision: 0 }, { kind: 'gauge', channel: 'rpm' });
  return p.finish();
}
export function axisRacing() {
  const p = panel('example-racing', 'Racing axes', ['Driving controls', 'Driving feedback'], ['bipolar', 'unipolar', 'unipolar', 'unipolar']);
  p.add('slider', 'steering', 'Steering', 0, { x: 0.04, y: 0.07, width: 0.55, height: 0.22 }, [axis('steering', 'Steering', 0, 'bipolar')], sliderConfig('horizontal'));
  p.add('slider', 'throttle', 'Throttle', 0, { x: 0.65, y: 0.07, width: 0.13, height: 0.49 }, [axis('throttle', 'Throttle', 1, 'unipolar')], sliderConfig('vertical'));
  p.add('slider', 'brake', 'Brake', 0, { x: 0.82, y: 0.07, width: 0.13, height: 0.49 }, [axis('brake', 'Brake', 2, 'unipolar')], sliderConfig('vertical'));
  p.add('slider', 'handbrake', 'Handbrake', 0, { x: 0.04, y: 0.35, width: 0.55, height: 0.22 }, [axis('handbrake', 'Handbrake', 3, 'unipolar')], sliderConfig('horizontal'));
  p.add('button', 'shift-down', 'Shift down (held)', 0, { x: 0.04, y: 0.67, width: 0.24, height: 0.23 }, [button('shift-down', 'Shift down', 0)], buttonConfig());
  p.add('button', 'shift-up', 'Shift up (held)', 0, { x: 0.34, y: 0.67, width: 0.25, height: 0.23 }, [button('shift-up', 'Shift up', 1)], buttonConfig());
  p.add('gauge', 'rpm', 'Engine RPM', 0, { x: 0.65, y: 0.67, width: 0.30, height: 0.23 }, [], { min: 0, max: 100000, unit: 'rpm', precision: 0 }, { kind: 'gauge', channel: 'rpm' });
  p.add('numeric', 'speed', 'Speed', 1, { x: 0.06, y: 0.08, width: 0.42, height: 0.34 }, [], { min: 0, max: 2000, unit: 'km/h', precision: 1 }, { kind: 'gauge', channel: 'speed' });
  p.add('numeric', 'gear', 'Gear', 1, { x: 0.52, y: 0.08, width: 0.42, height: 0.34 }, [], { min: -1, max: 31, unit: 'ratio', precision: 0 }, { kind: 'gauge', channel: 'gear' });
  p.add('gauge', 'steering-state', 'Steering requested', 1, { x: 0.06, y: 0.52, width: 0.88, height: 0.34 }, [], { min: -1, max: 1, unit: 'ratio', precision: 2 }, { kind: 'gauge', channel: 'steering-requested' });
  return p.finish();
}
export async function exampleBundles() {
  return new Map([
    ['flight-button-box.json', await exportBundle(flightButtonBox(), []) + '\n'],
    ['axis-racing.json', await exportBundle(axisRacing(), []) + '\n'],
  ]);
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  for (const [name, content] of await exampleBundles()) await writeFile(new URL(name, import.meta.url), content);
}
