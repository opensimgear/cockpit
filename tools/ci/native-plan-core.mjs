const projectsByFlag = {
  native_host: 'native-host',
  native_controllers: 'native-controllers',
  native_profiles: 'native-profiles',
  native_persistence: 'native-persistence',
  native_mapping: 'native-mapping',
  native_logic: 'native-logic',
  native_mobile: 'native-mobile',
  native_esp32: 'native-esp32',
  native_windows_package: 'native-windows-package',
};

export function planNative({ projects, full = false }) {
  const selected = new Set(projects);
  return Object.fromEntries(Object.entries(projectsByFlag).map(([flag, project]) => [
    flag,
    Boolean(full || selected.has(project)),
  ]));
}
