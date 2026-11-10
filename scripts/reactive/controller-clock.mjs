// Both probes serialize this function into the dev Cockpit WebView.
export async function calibrateControllerClock(
  gatewayRequest,
  invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args),
  now = () => performance.now(),
) {
  const direct = async () => invoke('gateway_request', {request: {command: 'qualification_controller_clock'}});
  const probe = await direct().catch(() => null);
  const method = typeof probe?.id === 'string' && typeof probe.nowUs === 'string' ? 'qualification' : 'inventory';
  const read = method === 'qualification' ? direct : async () =>
    (await gatewayRequest({command: 'controller_list', offset: 0})).controllers?.captureClock;
  const rows = [];
  for (let i = 0; i < 20; i++) {
    const before = now();
    const clock = await read();
    const after = now();
    const nativeUs = Number(clock?.nowUs);
    if (typeof clock?.id !== 'string' || !clock.id.startsWith('controllers:')
      || typeof clock.nowUs !== 'string' || !/^[1-9][0-9]*$/.test(clock.nowUs) || !Number.isSafeInteger(nativeUs)
      || (rows.length && clock.id !== rows[0].id) || !Number.isFinite(before) || !Number.isFinite(after) || after < before) {
      throw Error('Invalid controller capture clock sample.');
    }
    rows.push({before, after, native: nativeUs / 1000, id: clock.id, rtt: after - before});
  }
  const best = rows.reduce((a, b) => a.rtt < b.rtt ? a : b);
  // Allow for the Windows WebView's observed 0.1 ms and the journal's 1 µs rounding.
  return {rows, id: best.id, method, rtt: best.rtt,
    offset: (best.before + best.after) / 2 - best.native, uncertainty: best.rtt / 2 + 0.101};
}
