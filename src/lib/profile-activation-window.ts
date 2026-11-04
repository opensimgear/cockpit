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
type CancelCallback = () => void | boolean | Promise<unknown>;
type ObserveWindow = typeof import('./activation-window-observation').observeActivationWindow;
type WindowOperation = {
  operationId: string;
  progress: ActivationWindowProgress;
  cancel: CancelCallback;
  delivered: boolean;
  observationError: boolean;
};
interface WindowDependencies {
  invoke?: NativeInvoke;
  isNative?: () => boolean;
  observe: ObserveWindow;
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
  private readonly observe: ObserveWindow;
  private unsubscribe?: () => void;
  private active?: WindowOperation;
  private queue: Promise<unknown> = Promise.resolve();

  constructor(dependencies: WindowDependencies) {
    this.invoke = dependencies.invoke ?? nativeInvoke;
    this.isNative = dependencies.isNative ?? (() => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window);
    this.observe = dependencies.observe;
  }

  private enqueue<T>(action: () => Promise<T>): Promise<T> {
    const pending = this.queue.then(action, action);
    this.queue = pending.catch(() => undefined);
    return pending;
  }

  async show(progress: ActivationWindowProgress, onCancel: CancelCallback): Promise<boolean> {
    if (!this.isNative()) return false;
    this.stopObserving();
    const operation: WindowOperation = { operationId: progress.operationId, progress, cancel: onCancel, delivered: false, observationError: false };
    this.active = operation;
    try {
      await this.enqueue(() => this.invoke('profile_activation_window_open', { progress }));
      if (this.active !== operation) return false;
      const unsubscribe = this.observe((snapshot, error) => {
        if (this.active !== operation) return;
        operation.observationError = error !== undefined;
        if (!operation.observationError) void this.forwardCancel(operation, snapshot);
      });
      if (this.active !== operation) { unsubscribe(); return false; }
      this.unsubscribe = unsubscribe;
      if (operation.observationError) { await this.close(operation.operationId); return false; }
      return true;
    } catch {
      if (this.active === operation) {
        this.active = undefined;
        this.stopObserving();
        // Creation may have succeeded before observation failed. Close only this
        // exact operation so fallback never leaves a stale owned window open.
        await this.close(operation.operationId);
      }
      return false;
    }
  }

  async update(progress: ActivationWindowProgress): Promise<boolean> {
    const operation = this.active;
    if (operation?.operationId !== progress.operationId) return false;
    if (operation.observationError) { await this.close(operation.operationId); return false; }
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
      this.stopObserving();
    }
    try {
      await this.enqueue(() => this.invoke('profile_activation_window_close', { operationId }));
    } catch { /* The operation has finished even if its window was already destroyed. */ }
  }

  dispose(): void {
    const operationId = this.active?.operationId;
    this.active = undefined;
    this.stopObserving();
    if (operationId) void this.close(operationId);
  }

  private stopObserving(): void {
    this.unsubscribe?.();
    this.unsubscribe = undefined;
  }

  private async forwardCancel(
    operation: WindowOperation,
    snapshot: ActivationWindowSnapshot | null,
  ): Promise<void> {
    if (this.active !== operation || operation.delivered || snapshot?.progress.operationId !== operation.operationId
      || !snapshot.cancelRequested || !profileProgressCanCancel(operation.progress) || !profileProgressCanCancel(snapshot.progress)) return;
    operation.delivered = true;
    try {
      if (await operation.cancel() !== false) return;
    } catch { /* Release a failed intent so the user can retry. */ }
    try { await this.resetFailedCancel(operation); }
    catch { /* The native view can still receive a later progress update. */ }
  }

  private async resetFailedCancel(operation: WindowOperation): Promise<void> {
    if (this.active !== operation) return;
    await this.enqueue(() => this.invoke('profile_activation_window_update', { progress: operation.progress, resetCancelRequest: true }));
    operation.delivered = false;
  }
}
