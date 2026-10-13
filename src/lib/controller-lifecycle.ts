import type { GatewayModule, GatewayState } from './gateway';

export type ControllerAction = 'enable' | 'disable' | 'forget' | 'update-profile';
export const bindingWarning = 'This reconnects the gateway and may require rebinding remaining controllers in your games.';

export function automaticControllers(state: GatewayState | null | undefined): boolean {
  return state?.backend === 'stm32' && state.protocolVersion === 3 && state.lifecycle === 'automatic';
}

/** Recheck live state when confirming a dialog, rather than trusting its target. */
export function canChangeController(state: GatewayState | null | undefined, module: GatewayModule | undefined, action: ControllerAction): boolean {
  if (!automaticControllers(state) || !state?.connected || !module || state.reconnecting || state.layoutPending || state.claimedModules?.length) return false;
  if (action === 'forget') return !module.online;
  if (action === 'enable') return module.enabled === false;
  if (action === 'disable') return module.enabled === true;
  return module.enabled === true && module.online && module.profileMatch === false;
}

export function controllerWarning(module: GatewayModule | undefined, action: ControllerAction): string | undefined {
  if ((action === 'disable' || action === 'forget' || action === 'update-profile') && module?.published) return bindingWarning;
}

export function controllerStatus(module: GatewayModule | undefined): string {
  if (!module) return 'Unavailable';
  if (!module.enabled) return module.published ? 'Disable pending · controller retained' : 'Disabled · remembered';
  if (!module.published) return 'Enabling';
  if (!module.online) return 'Offline · controller retained';
  if (!module.profileMatch) return 'Profile changed';
  return 'Enabled';
}

/** Published collections first, in their actual descriptor order; disabled
 * records retain inventory order and never get a fabricated controller index. */
export function orderedModules(state: GatewayState | null | undefined): GatewayModule[] {
  const modules = state?.modules ?? [];
  if (!automaticControllers(state)) return modules;
  return [...modules].sort((a, b) => (a.published ? a.descriptorIndex! : Infinity) - (b.published ? b.descriptorIndex! : Infinity));
}
