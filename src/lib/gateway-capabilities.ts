import type { GatewayState } from './gateway';

/** Current Pico Debug firmware implements the same device contracts as CAN gateways. */
export function supportsDeviceContracts(state: GatewayState | null | undefined): boolean {
  return state?.backend === 'stm32' || (state?.backend === 'pico' && state.protocolVersion === 3 && state.canProtocol === 3);
}
