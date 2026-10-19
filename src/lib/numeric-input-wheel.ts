/** Apply wheel edits through the same input/change handlers as typed edits. */
export function installNumericInputWheel(root: HTMLElement): () => void {
  function wheel(event: WheelEvent) {
    const input = event.target;
    if (!(input instanceof HTMLInputElement) || input.type !== 'number' || input.matches(':disabled') || input.readOnly
      || input.closest('[inert]') || !event.deltaY || event.metaKey) return;
    event.preventDefault();
    event.stopPropagation();
    const minimum = input.min === '' ? -Infinity : Number(input.min);
    const maximum = input.max === '' ? Infinity : Number(input.max);
    const current = Number.isFinite(input.valueAsNumber) ? input.valueAsNumber : 0;
    const next = Math.max(minimum, Math.min(maximum, current + (event.deltaY < 0 ? 1 : -1) * (event.shiftKey || event.ctrlKey ? 10 : 1)));
    if (!Number.isFinite(next) || next === input.valueAsNumber) return;
    input.valueAsNumber = next;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
  }
  root.addEventListener('wheel', wheel, { capture: true, passive: false });
  return () => root.removeEventListener('wheel', wheel, { capture: true });
}
