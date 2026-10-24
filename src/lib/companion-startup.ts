import type { CompanionHostState, CompanionSend } from './companions.ts';

/** Native SQLite restores content before transports. Frontend checks cannot
 * republish authoring archives, establish heads or recreate assignments. */
export async function readNativeCompanionState(send: CompanionSend): Promise<CompanionHostState> {
  const state = await send('companion_status');
  if (!state.host) throw new Error('Companion content requires the native Cockpit.');
  return state;
}
let checking: Promise<void> | undefined;
/** Explicit designer operations wait for the native owner; app mount does not
 * initialize first-time trust or TLS merely to replay a frontend workspace. */
export function restoreCompanionPanels(): Promise<void> {
  return checking ??= (async () => {
    const { gatewayRequest } = await import('./gateway');
    await readNativeCompanionState(async (command, companion = {}) => {
      const state = (await gatewayRequest({ command, companion })).companions;
      if (!state) throw new Error('Companion content requires the native Cockpit.');
      return state;
    });
  })().catch(error => { checking = undefined; throw error; });
}
