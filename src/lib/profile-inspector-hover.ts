/** Auto-hide presentation only; the selected block and its form stay mounted. */
export function profileInspectorHover(panel: HTMLElement) {
  const content = panel.querySelector<HTMLElement>('[data-inspector-content]');
  const reveal = panel.querySelector<HTMLButtonElement>('[data-inspector-reveal]');
  if (!content || !reveal) return;
  const listeners = new AbortController();
  const { signal } = listeners;
  let hovered = panel.matches(':hover');
  let keyboard = false;
  let keyboardFocus = false;
  let touchOpen = false;
  let nativeSelect = false;
  let touchClick = false;

  function update() {
    const expanded = hovered || keyboardFocus || touchOpen || nativeSelect;
    panel.dataset.expanded = String(expanded);
    content!.inert = !expanded;
    content!.setAttribute('aria-hidden', String(!expanded));
    reveal!.setAttribute('aria-expanded', String(expanded));
  }
  panel.ownerDocument.addEventListener('keydown', event => {
    if (event.key === 'Tab') keyboard = true;
  }, { capture: true, signal });
  panel.ownerDocument.addEventListener('pointerdown', event => {
    keyboard = false;
    keyboardFocus = false;
    if (!panel.contains(event.target as Node)) {
      touchOpen = false;
      nativeSelect = false;
      hovered = false;
      update();
    }
  }, { capture: true, signal });
  panel.addEventListener('pointerenter', event => {
    if (event.pointerType === 'touch') return;
    hovered = true;
    update();
  }, { signal });
  panel.addEventListener('pointerleave', event => {
    if (event.pointerType === 'touch') return;
    hovered = false;
    // Native select menus can leave the WebView without moving to another DOM
    // element. Keep the menu usable until its selection or focus completes.
    nativeSelect = event.relatedTarget === null && panel.ownerDocument.activeElement instanceof HTMLSelectElement;
    if (!keyboard) keyboardFocus = false;
    update();
  }, { signal });
  panel.addEventListener('focusin', () => {
    if (keyboard) keyboardFocus = true;
    update();
  }, { signal });
  panel.addEventListener('focusout', event => {
    if (event.relatedTarget instanceof Node && panel.contains(event.relatedTarget)) return;
    keyboardFocus = false;
    nativeSelect = false;
    update();
  }, { signal });
  panel.addEventListener('change', () => {
    nativeSelect = false;
    hovered = panel.matches(':hover');
    update();
  }, { signal });
  panel.addEventListener('keydown', event => {
    if (event.key === 'Escape') {
      nativeSelect = false;
      hovered = panel.matches(':hover');
      update();
    }
  }, { signal });
  reveal.addEventListener('pointerdown', event => { touchClick = event.pointerType === 'touch'; }, { signal });
  reveal.addEventListener('click', event => {
    if (event.detail !== 0 && (touchClick || !matchMedia('(hover: hover) and (pointer: fine)').matches)) touchOpen = !touchOpen;
    update();
  }, { signal });
  update();
  return { destroy() { listeners.abort(); } };
}
