export function evaluateDailyGate({ hasChanges, flags, results }) {
  const enabled = (name) => flags[name] === true || flags[name] === 'true';
  const failures = [];
  for (const required of ['baseline']) {
    if (results[required] !== 'success') failures.push(`${required} job was ${results[required] ?? 'missing'}`);
  }
  if (!hasChanges) return { success: failures.length === 0, failures };
  if (results.selection !== 'success') failures.push(`selection job was ${results.selection ?? 'missing'}`);

  const nativeSelected = ['native_host', 'native_controllers', 'native_profiles', 'native_persistence', 'native_mapping', 'native_logic']
    .some(enabled);
  const expected = {
    native: nativeSelected,
    mobile: enabled('native_mobile'),
    esp32: enabled('native_esp32'),
    windows_package: enabled('native_windows_package'),
  };
  for (const [job, selected] of Object.entries(expected)) {
    const deferredEsp32 = job === 'esp32' && results[job] === 'skipped';
    if (selected && results[job] !== 'success' && !deferredEsp32) {
      failures.push(`${job} job was ${results[job] ?? 'missing'}`);
    }
  }
  return { success: failures.length === 0, failures };
}
