import { dcsInputInterfaces, dcsOutputActions, dcsOutputIndex, physicalDcsControls, type DcsCatalogueControl, type DcsInputBinding, type DcsOutputBinding, type DcsProfileConfig, type DcsTarget } from './dcs-profile';

export const dcsTestPresetQueries = {
  'master-caution': ['UFC_MASTER_CAUTION', 'MASTER_CAUTION'],
  radio: ['UHF_TEST', 'UHF_SQUELCH', 'UHF_PRESET_SEL', 'UHF_10MHZ_SEL', 'UHF_VOL', 'UHF_FREQUENCY', 'UHF_PRESET', 'MASTER_CAUTION'],
} as const;
export type DcsTestPreset = keyof typeof dcsTestPresetQueries;

/** Add a complete reviewable preset only when the installed catalogue and
 * current device advertise every required input/output. No wire addresses are
 * guessed, and existing bindings cannot be silently replaced. */
export function dcsTestPreset(profile: DcsProfileConfig, target: DcsTarget, kind: DcsTestPreset,
  catalogue: DcsCatalogueControl[]): { inputs: DcsInputBinding[]; outputs: DcsOutputBinding[]; error: string } {
  const inputs: DcsInputBinding[] = [], outputs: DcsOutputBinding[] = [];
  const events = target.state?.moduleEvents?.[target.module.id], actions = target.state?.moduleActions?.[target.module.id];
  const fail = (error: string) => ({ inputs: [], outputs: [], error });
  if (!target.module.online || !target.module.boot || events?.boot !== target.module.boot || actions?.boot !== target.module.boot) return fail('Refresh this online device’s input and output capabilities before adding a preset.');
  const ids = new Set([...profile.inputs, ...profile.outputs].map(binding => binding.id));
  function nextId(prefix: string): string {
    let index = 1; while (ids.has(`${prefix}-${index}`)) index++;
    const id = `${prefix}-${index}`; ids.add(id); return id;
  }
  const source = { gatewayId: target.gatewayId, moduleId: target.module.id };
  const mappings: [DcsInputBinding['kind'], number, string, DcsInputBinding['interface']][] = kind === 'master-caution'
    ? [['button', 0, 'UFC_MASTER_CAUTION', 'set_state']]
    : [['button', 0, 'UHF_TEST', 'set_state'], ['switch', 0, 'UHF_SQUELCH', 'set_state'],
      ['encoder', 0, 'UHF_PRESET_SEL', 'fixed_step'], ['encoder', 1, 'UHF_10MHZ_SEL', 'fixed_step'], ['axis', 0, 'UHF_VOL', 'set_state']];
  for (const [inputKind, ordinal, command, iface] of mappings) {
    const control = physicalDcsControls(inputKind, events)[ordinal];
    if (!control) return fail(`This preset needs ${inputKind} ${ordinal + 1}. Choose the matching Pico panel preset.`);
    const definition = catalogue.find(control => control.id === command);
    if (!dcsInputInterfaces(inputKind, definition).includes(iface)) return fail(`The installed catalogue does not advertise ${command} with ${iface}.`);
    if (profile.inputs.some(binding => binding.command === command || binding.gatewayId === source.gatewayId && binding.moduleId === source.moduleId && binding.kind === inputKind && binding.control === control.value)) return fail(`${command} or its device input is already bound. Review existing bindings first.`);
    inputs.push({ ...source, id: nextId(`${kind}-${inputKind}`), kind: inputKind, control: control.value, command,
      interface: iface, pressedValue: 1, releasedValue: 0, step: 1, ...(inputKind === 'axis' ? { deadband: 256 } : {}) });
  }
  const outputMappings: [string, DcsOutputBinding['format'], string][] = kind === 'master-caution'
    ? [['MASTER_CAUTION', { kind: 'boolean' }, 'led']]
    : [['UHF_FREQUENCY', { kind: 'text', unknownText: '???????', staleText: '-------', overflow: 'truncate' }, 'frequency'],
      ['UHF_PRESET', { kind: 'text', unknownText: '??', staleText: '--', overflow: 'truncate' }, 'preset'],
      ['UHF_VOL', { kind: 'fixed', scale: 100 / 65535, decimals: 1, prefix: 'VOL ', unknownText: 'UNKNOWN', staleText: 'STALE', overflow: 'truncate' }, 'text'],
      ['MASTER_CAUTION', { kind: 'boolean' }, 'led']];
  for (const [control, format, endpoint] of outputMappings) {
    const definition = catalogue.find(item => item.id === control);
    if (!definition?.outputs.some(output => output.type === (format.kind === 'text' ? 'string' : 'integer'))) return fail(`The installed catalogue does not advertise compatible ${control} feedback.`);
    const available = dcsOutputActions(format.kind, actions);
    const action = endpoint === 'led' ? available[0] : available.find(action => `${action.id}/${action.endpoint}`.includes(`.${endpoint}.`) || action.endpoint === endpoint);
    if (!action) return fail(`This device does not advertise a compatible ${endpoint} output. Discover its actions first.`);
    if (profile.outputs.some(binding => binding.gatewayId === source.gatewayId && binding.moduleId === source.moduleId && binding.actionId === action.id)) return fail(`${action.label} is already bound. Review existing outputs first.`);
    outputs.push({ ...source, id: nextId(`${kind}-${endpoint}`), control, output: dcsOutputIndex(format.kind, definition), actionId: action.id, format });
  }
  if (profile.inputs.length + inputs.length > 16 || profile.outputs.length + outputs.length > 16) return fail('This preset would exceed the profile limit of 16 input and 16 output bindings.');
  return { inputs, outputs, error: '' };
}
