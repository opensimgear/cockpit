import type { GatewayBackend, GatewayPort, GatewayState } from './gateway';

export function candidateBackend(port: GatewayPort): GatewayBackend | undefined {
  if (port.vendorId?.toLowerCase().replace(/^0x/, '') === 'cafe' &&
      port.productId?.toLowerCase().replace(/^0x/, '') === '4010' &&
      /^OSG-PICO-[A-Za-z0-9_-]{4,64}$/.test(port.serialNumber ?? '')) return 'pico';
  const uid = port.serialNumber ?? '';
  if (/^[a-f0-9]{24}$/i.test(uid) && !/^0{24}$/.test(uid) && !/^f{24}$/i.test(uid) &&
      /^opensimgear$/i.test(port.manufacturer ?? '') && /gateway/i.test(port.friendlyName ?? '')) return 'stm32';
  return undefined;
}

export function gatewayCandidates(ports: GatewayPort[] = []): GatewayPort[] {
  const compatible = ports.flatMap(port => {
    const backend = candidateBackend(port);
    return backend ? [{ ...port, backend }] : [];
  });
  // macOS call-in and call-out aliases share the same USB interface suffix.
  const byInterface = new Map<string, GatewayPort>();
  for (const port of compatible) {
    const key = port.path.replace(/^\/dev\/(?:cu|tty)\.usbmodem/, '/dev/usbmodem');
    const existing = byInterface.get(key);
    if (!existing || port.path.startsWith('/dev/cu.')) byInterface.set(key, port);
  }
  return [...byInterface.values()].sort((a, b) => Number(b.backend === 'stm32') - Number(a.backend === 'stm32') || a.path.localeCompare(b.path));
}

export function automaticGatewayAction(
  state: GatewayState | null,
  options: { paused: boolean; setupActive: boolean; failedPorts: Set<string> }
): { command?: 'connect' | 'start'; port?: string; backend?: GatewayBackend; reason: string } {
  if (options.paused) return { reason: 'Automatic startup failed. Retry the gateway or unplug and reconnect it.' };
  if (!state) return { reason: 'Looking for an OpenSimGear gateway…' };
  if (state.reconnecting) return { reason: 'Waiting for USB reconnection.' };
  if (options.setupActive || state.claimedModules?.length) return { reason: 'Waiting for setup access to be released.' };
  if (!state.connected) {
    const candidates = gatewayCandidates(state.ports);
    if (!candidates.length) return { reason: 'Waiting for an OpenSimGear CAN gateway or Pico simulator.' };
    const candidate = candidates.find(port => !options.failedPorts.has(port.path));
    if (!candidate) return { reason: 'Automatic connection failed. Check the error and retry discovery, or unplug and reconnect the Pico.' };
    return { command: 'connect', port: candidate.path, backend: candidate.backend, reason: 'Connecting to your OpenSimGear gateway…' };
  }
  if (state.backend === 'stm32') return { reason: state.needsReconnect ? 'Discovered devices are listed below. Apply the pending controller layout when ready.' : 'CAN discovery is active. Connected devices appear automatically.' };
  if (state.running) return { reason: 'Connected automatically. USB HID inputs are ready.' };
  if (state.needsReconnect) return { reason: 'Reconnect the gateway to publish pending controller changes before starting.' };
  if (!state.modules.some(module => module.online && module.registered)) return { reason: 'No registered online modules to start.' };
  return { command: 'start', reason: 'Starting your gateway…' };
}
