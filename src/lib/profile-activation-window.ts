export interface ActivationWindowProgress {
  operationId: string;
  profileName: string;
  title: string;
  phase: 'preparing' | 'activating' | 'cancelling' | 'launching' | 'deactivating' | 'recovering' | 'switching';
  message: string;
  details: string[];
  canCancel: boolean;
}

export interface ActivationWindowSnapshot {
  progress: ActivationWindowProgress;
  cancelRequested: boolean;
}

/** Cleanup must finish even if a late close or cancellation intent is queued. */
export const profileProgressCanCancel = (progress: Pick<ActivationWindowProgress, 'phase' | 'canCancel'>): boolean =>
  progress.canCancel && !['deactivating', 'recovering', 'cancelling', 'launching', 'switching'].includes(progress.phase);

type NativeInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type CancelCallback = () => void | Promise<unknown>;
interface WindowDependencies {
  invoke?: NativeInvoke;
  isNative?: () => boolean;
  setInterval?: typeof globalThis.setInterval;
  clearInterval?: typeof globalThis.clearInterval;
}

const nativeInvoke: NativeInvoke = async (command, args) => {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke(command, args);
};

/** Only the parent controller owns activation. This adapter forwards cancellation
 * intent for the exact displayed operation; the child has no runtime authority. */
export class NativeProfileActivationWindow {
  private readonly invoke: NativeInvoke;
  private readonly isNative: () => boolean;
  private readonly schedule: typeof globalThis.setInterval;
  private readonly unschedule: typeof globalThis.clearInterval;
  private timer?: ReturnType<typeof globalThis.setInterval>;
  private active?: { operationId: string; progress: ActivationWindowProgress; cancel: CancelCallback; delivered: boolean };
  private queue: Promise<unknown> = Promise.resolve();
  private polling = false;

  constructor(dependencies: WindowDependencies = {}) {
    this.invoke = dependencies.invoke ?? nativeInvoke;
    this.isNative = dependencies.isNative ?? (() => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window);
    // WebView2 timers require their Window receiver. Calling an unbound timer
    // as an adapter method throws after the native window has already opened.
    this.schedule = dependencies.setInterval ?? globalThis.setInterval.bind(globalThis);
    this.unschedule = dependencies.clearInterval ?? globalThis.clearInterval.bind(globalThis);
  }

  private enqueue<T>(action: () => Promise<T>): Promise<T> {
    const pending = this.queue.then(action, action);
    this.queue = pending.catch(() => undefined);
    return pending;
  }

  async show(progress: ActivationWindowProgress, onCancel: CancelCallback): Promise<boolean> {
    if (!this.isNative()) return false;
    this.stopPolling();
    const operation = { operationId: progress.operationId, progress, cancel: onCancel, delivered: false };
    this.active = operation;
    try {
      await this.enqueue(() => this.invoke('profile_activation_window_open', { progress }));
      if (this.active !== operation) return false;
      this.timer = this.schedule(() => { void this.poll(operation); }, 150);
      return true;
    } catch {
      if (this.active === operation) {
        this.active = undefined;
        this.stopPolling();
        // Creation may have succeeded before polling failed. Close only this
        // exact operation so fallback never leaves a stale owned window open.
        await this.close(operation.operationId);
      }
      return false;
    }
  }

  async update(progress: ActivationWindowProgress): Promise<boolean> {
    const operation = this.active;
    if (operation?.operationId !== progress.operationId) return false;
    operation.progress = progress;
    let updated = false;
    try {
      updated = await this.enqueue(() => this.invoke<boolean>('profile_activation_window_update', { progress }));
    } catch { /* The parent will display its local progress dialog instead. */ }
    if (!updated && this.active === operation) await this.close(operation.operationId);
    return updated;
  }

  async close(operationId: string): Promise<void> {
    if (!this.isNative()) return;
    if (this.active?.operationId === operationId) {
      this.active = undefined;
      this.stopPolling();
    }
    try {
      await this.enqueue(() => this.invoke('profile_activation_window_close', { operationId }));
    } catch { /* The operation has finished even if its window was already destroyed. */ }
  }

  dispose(): void {
    const operationId = this.active?.operationId;
    this.active = undefined;
    this.stopPolling();
    if (operationId) void this.close(operationId);
  }

  private stopPolling(): void {
    if (this.timer !== undefined) this.unschedule(this.timer);
    this.timer = undefined;
  }

  private async poll(operation: NonNullable<NativeProfileActivationWindow['active']>): Promise<void> {
    if (this.polling || this.active !== operation || operation.delivered || !profileProgressCanCancel(operation.progress)) return;
    this.polling = true;
    try {
      const snapshot = await this.invoke<ActivationWindowSnapshot | null>('profile_activation_window_state');
      if (this.active !== operation || snapshot?.progress.operationId !== operation.operationId || !snapshot.cancelRequested
        || !profileProgressCanCancel(operation.progress) || !profileProgressCanCancel(snapshot.progress)) return;
      operation.delivered = true;
      try {
        if (await operation.cancel() === false) await this.resetFailedCancel(operation);
      } catch { await this.resetFailedCancel(operation); }
    } catch { /* An update can recover from a transient native read failure. */ }
    finally { this.polling = false; }
  }

  private async resetFailedCancel(operation: NonNullable<NativeProfileActivationWindow['active']>): Promise<void> {
    if (this.active !== operation) return;
    await this.enqueue(() => this.invoke('profile_activation_window_update', { progress: operation.progress, resetCancelRequest: true }));
    operation.delivered = false;
  }
}
