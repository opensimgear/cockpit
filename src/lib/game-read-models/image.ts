import { observeGameAsset } from './shared.ts';
import type { GameListener } from './models.ts';
type ObserveAsset = (reference: string, listener: GameListener<string>) => () => void;
/** Opaque references must never become browser URLs, including before an action mounts. */
export function gameImageSource(reference: string | undefined | null): string | undefined {
  return reference && !reference.startsWith('game-asset:') ? reference : undefined;
}
export function resolveGameImage(reference: string, signal?: AbortSignal, observe: ObserveAsset = observeGameAsset): Promise<string> {
  if (signal?.aborted) return Promise.reject(new Error('Image selection was cancelled.'));
  if (!reference.startsWith('game-asset:')) return Promise.resolve(reference);
  return new Promise((resolve, reject) => {
    let stop: (() => void) | undefined, done = false;
    const finish = (value?: string, error?: unknown) => {
      if (done) return; done = true; signal?.removeEventListener('abort', abort); stop?.();
      if (error) reject(error); else resolve(value!);
    };
    const abort = () => finish(undefined, new Error('Image selection was cancelled.'));
    signal?.addEventListener('abort', abort, { once: true });
    try {
      stop = observe(reference, state => {
        if (state.status === 'ready' && state.value) finish(state.value);
        else if (state.status === 'error') finish(undefined, new Error(state.error || 'The game image could not be loaded.'));
      });
      if (done) stop();
    } catch (error) { finish(undefined, error); }
    if (signal?.aborted) abort();
  });
}
/** A single intersection observer gates every game image in the mounted document. */
export function createGameImageAction(observe: ObserveAsset = observeGameAsset, intersection = () => typeof IntersectionObserver === 'undefined' ? undefined : IntersectionObserver) {
  const waiting = new Map<HTMLImageElement, () => void>();
  let observer: IntersectionObserver | undefined;
  function unwatch(node: HTMLImageElement) {
    waiting.delete(node); observer?.unobserve(node);
    if (!waiting.size) { observer?.disconnect(); observer = undefined; }
  }
  function visible(node: HTMLImageElement, load: () => void) {
    const Observer = intersection();
    if (!Observer) { load(); return; }
    observer ??= new Observer(entries => { for (const entry of entries) if (entry.isIntersecting) { const start = waiting.get(entry.target as HTMLImageElement); unwatch(entry.target as HTMLImageElement); start?.(); } }, { rootMargin: '200px' });
    waiting.set(node, load); observer.observe(node);
  }
  return (node: HTMLImageElement, initial: string | undefined | null) => {
    let stop: (() => void) | undefined, generation = 0, disposed = false;
    const clear = () => { generation++; stop?.(); stop = undefined; unwatch(node); };
    const update = (reference: string | undefined | null) => {
      clear(); const current = generation;
      const direct = gameImageSource(reference);
      if (direct) { node.src = direct; node.dataset.gameImageState = 'ready'; return; }
      node.removeAttribute('src');
      if (!reference) { delete node.dataset.gameImageState; return; }
      node.dataset.gameImageState = 'loading';
      visible(node, () => {
        if (disposed || current !== generation) return;
        stop = observe(reference, state => {
          if (disposed || current !== generation) return;
          if (state.status === 'ready' && state.value) { node.src = state.value; node.dataset.gameImageState = 'ready'; }
          else if (state.status === 'error') { node.dataset.gameImageState = 'error'; node.dispatchEvent(new Event('error')); }
        });
      });
    };
    update(initial);
    return { update, destroy() { disposed = true; clear(); } };
  };
}
export const gameImage = createGameImageAction();
