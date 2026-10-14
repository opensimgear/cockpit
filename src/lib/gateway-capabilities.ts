import type { GatewayState } from './gateway';

/** The Pico debug gateway implements the same device contracts as CAN gateways.
 * Protocol-1 Pico fixtures retain their original limited configuration path. */
export function supportsDeviceContracts(state: GatewayState | null | undefined): boolean {
  return state?.backend === 'stm32' || (state?.backend === 'pico' && state.protocolVersion === 3 && state.canProtocol === 3);
}
