import type { GatewayRequest, GatewayModule, GatewayState } from './gateway';
import type { RuntimeConfig } from './runtime-config';

export type DebugOperation = 'contact' | 'axis' | 'turn' | 'hat' | 'selector' | 'neutral' | 'online' | 'reboot' | 'direct' | 'expansion' | 'add';
export type DebugDevice = {
  id: string; preset: string; online: boolean; boot: number; directUsb: boolean;
  contacts: number[]; axes: number[]; expansionMask: number; definition: RuntimeConfig;
};
export type DebugGateway = { maxDevices: number; presets: string[]; devices: DebugDevice[] };
export const debugPresetLabels: Record<string, string> = {
  shifter: 'Sequential shifter', pedals: 'Pedals', handbrake: 'Handbrake', buttons: 'Button panel',
  'master-caution': 'DCS master-caution panel', radio: 'DCS radio panel', 'all-inputs': 'All input types',
};
export function debugReady(state: GatewayState | null, module: GatewayModule | undefined): boolean {
  const device = state?.debug?.devices.find(device => device.id === module?.id);
  return !!state?.connected && !state.reconnecting && !state.layoutPending && !!device && !!module &&
    (!device.online || device.boot === module.boot);
}
export function debugDefinition(state: GatewayState, device: DebugDevice): RuntimeConfig {
  const snapshot = state.moduleConfigs?.[device.id];
  // A setup draft is not yet the configuration used to acquire device input.
  return snapshot?.view === 'active' ? snapshot.config : device.definition;
}
export function rawAxisValue(value: string | number): number | undefined {
  if (typeof value === 'string' && !value.trim()) return;
  const numeric = Number(value);
  return Number.isInteger(numeric) && numeric >= 0 && numeric <= 4095 ? numeric : undefined;
}

type Pending = { request: GatewayRequest; axis?: number; done: ((result: boolean) => void)[] };

/** One ordered input stream. Only pending axes after the last discrete edge may
 * coalesce; press/release and encoder detents always retain their order. */
export class DebugInputQueue {
  private pending: Pending[] = [];
  private running = false;
  private closed = false;
  private held = new Set<number>();
  constructor(private readonly target: { moduleId: string; boot: number },
    private readonly send: (request: GatewayRequest) => Promise<boolean>) {}

  enqueue(operation: DebugOperation, args: number[] = []): Promise<boolean> {
    if (this.closed) return Promise.resolve(false);
    const request: GatewayRequest = { command: 'debug_invoke', ...this.target, debugOperation: operation, debugArgs: [...args] };
    return new Promise(resolve => {
      if (operation === 'axis') {
        for (let i = this.pending.length - 1; i >= 0 && this.pending[i].axis !== undefined; i--) {
          if (this.pending[i].axis === args[0]) {
            this.pending[i].request = request; this.pending[i].done.push(resolve); return;
          }
        }
      }
      this.pending.push({ request, axis: operation === 'axis' ? args[0] : undefined, done: [resolve] });
      void this.drain();
    });
  }
  private async drain() {
    if (this.running) return;
    this.running = true;
    try {
      while (this.pending.length) {
        const next = this.pending.shift()!;
        let result = false;
        try { result = await this.send(next.request); } catch { /* The sender reports transport errors. */ }
        for (const done of next.done) done(result);
      }
    } finally { this.running = false; }
  }
  press(contact: number): Promise<boolean> {
    if (this.closed || this.held.has(contact)) return Promise.resolve(false);
    this.held.add(contact);
    return this.enqueue('contact', [contact, 1]);
  }
  release(contact: number): Promise<boolean> {
    if (!this.held.delete(contact)) return Promise.resolve(false);
    return this.enqueue('contact', [contact, 0]);
  }
  releaseHeld(): Promise<boolean[]> { return Promise.all([...this.held].map(contact => this.release(contact))); }
  reset(): Promise<boolean> {
    // Do not let an unsent slider sample override the explicit reset.
    this.pending = this.pending.filter(item => {
      if (item.axis === undefined) return true;
      for (const done of item.done) done(false);
      return false;
    });
    this.held.clear();
    return this.enqueue('neutral');
  }
  close(neutralize = true): Promise<boolean> {
    if (!neutralize) {
      this.closed = true; this.held.clear();
      for (const item of this.pending.splice(0)) for (const done of item.done) done(false);
      return Promise.resolve(false);
    }
    const result = this.reset(); this.closed = true; return result;
  }
}
