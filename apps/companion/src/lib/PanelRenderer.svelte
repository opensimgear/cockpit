<script lang="ts">
  import { createPreviewRecorder } from './preview-recorder.mjs';
  import { exampleUi as ui } from '@opensimgear/companion-contracts';
  export let recorder: ReturnType<typeof createPreviewRecorder>;
  export let snapshot: ReturnType<typeof recorder.snapshot>;
  export let refresh: () => void;
  const hats = [{ label: 'Up', arrow: '↑', value: 0 }, { label: 'Right', arrow: '→', value: 2 }, { label: 'Down', arrow: '↓', value: 4 }, { label: 'Left', arrow: '←', value: 6 }];
  function label(id: string) { return ui.controls.find((control) => control.id === id)?.label ?? id; }
  function capture(event: PointerEvent) {
    try { (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId); }
    catch { recorder.release(event.pointerId, 'Pointer capture unavailable'); }
    refresh();
  }
  function press(event: PointerEvent, id: string, kind: 'button' | 'hat', value: number | boolean) {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    event.preventDefault();
    if (recorder.press(event.pointerId, id, [{ id, kind, value }])) capture(event);
    refresh();
  }
  function release(event: PointerEvent, reason = 'Pointer released') { recorder.release(event.pointerId, reason); refresh(); }
  function xy(event: PointerEvent) {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const clamp = (value: number) => Math.round(Math.max(-1, Math.min(1, value)) * 1000) / 1000;
    return [{ id: 'steering-x', kind: 'axis' as const, value: clamp((event.clientX - rect.left) / rect.width * 2 - 1) }, { id: 'steering-y', kind: 'axis' as const, value: clamp(1 - (event.clientY - rect.top) / rect.height * 2) }];
  }
  function padDown(event: PointerEvent) {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    event.preventDefault();
    if (recorder.press(event.pointerId, 'xy-pad', xy(event))) capture(event);
    refresh();
  }
  function padMove(event: PointerEvent) { recorder.move(event.pointerId, xy(event)); refresh(); }
  function key(event: KeyboardEvent, id: string, value: boolean, kind: 'button' | 'hat' = 'button', hatValue = -1) {
    if (![' ', 'Enter'].includes(event.key)) return;
    event.preventDefault();
    const pointer = kind === 'hat' ? -10 - hatValue : id === 'primary' ? -1 : -2;
    if (value && !event.repeat) recorder.press(pointer, id, [{ id, kind, value: kind === 'button' ? true : hatValue }]);
    if (!value) recorder.release(pointer, 'Key released');
    refresh();
  }
</script>

<div class="panel-surface" class:disarmed={!snapshot.armed}>
  <div class="surface-heading"><div><span class="eyebrow">REFERENCE PANEL / 01</span><h2>{ui.title}</h2></div><span class="panel-mode">{snapshot.armed ? 'LOCAL PREVIEW' : 'DISARMED'}</span></div>
  <div class="control-grid">
    <section class="control-block pad-block" aria-label="Two-axis control">
      <div class="control-heading"><span>XY control</span><span class="control-hint">drag · spring return</span></div>
      <div class="xy-pad" role="group" aria-label="XY pad. Drag to record two axes. Returns to centre when released."
        onpointerdown={padDown} onpointermove={padMove} onpointerup={release}
        onpointercancel={(event) => release(event, 'Pointer cancelled')}
        onlostpointercapture={(event) => release(event, 'Lost pointer capture')}>
        <span class="axis-label top">Y +</span><span class="axis-label left">− X</span><span class="axis-label right">X +</span><span class="axis-label bottom">Y −</span>
        <span class="pad-cross horizontal"></span><span class="pad-cross vertical"></span>
        <span class="pad-ring"></span><span class="pad-position" style:left={`${50 + Number(snapshot.values['steering-x'] ?? 0) * 42}%`} style:top={`${50 - Number(snapshot.values['steering-y'] ?? 0) * 42}%`}></span>
      </div>
      <div class="axis-values"><span>{label('steering-x')} <strong>{Number(snapshot.values['steering-x'] ?? 0).toFixed(3)}</strong></span><span>{label('steering-y')} <strong>{Number(snapshot.values['steering-y'] ?? 0).toFixed(3)}</strong></span></div>
    </section>
    <section class="control-block action-block" aria-label="Momentary buttons and directional hat">
      <div class="control-heading"><span>Momentary controls</span><span class="control-hint">hold together</span></div>
      <div class="action-buttons">
        {#each ['primary', 'secondary'] as id, i}
          <button class="hold-button" class:pressed={snapshot.values[id] === true} disabled={!snapshot.armed}
            onpointerdown={(event) => press(event, id, 'button', true)} onpointerup={release}
            onpointercancel={(event) => release(event, 'Pointer cancelled')} onlostpointercapture={(event) => release(event, 'Lost pointer capture')}
            onkeydown={(event) => key(event, id, true)} onkeyup={(event) => key(event, id, false)} onblur={() => { recorder.release(id === 'primary' ? -1 : -2, 'Control lost focus'); refresh(); }}>
            <span class="button-number">0{i + 1}</span><span>{label(id)}</span><small>{snapshot.values[id] ? 'HELD' : 'HOLD'}</small>
          </button>
        {/each}
      </div>
      <div class="hat-container"><span class="control-hint">{label('view-hat')}</span><div class="hat-grid" aria-label="Four-way hat">
        {#each hats as direction}
          <button class={`hat-button hat-${direction.label.toLowerCase()}`} class:pressed={snapshot.values['view-hat'] === direction.value} disabled={!snapshot.armed}
            aria-label={`Hat ${direction.label}`} onpointerdown={(event) => press(event, 'view-hat', 'hat', direction.value)} onpointerup={release}
            onpointercancel={(event) => release(event, 'Pointer cancelled')} onlostpointercapture={(event) => release(event, 'Lost pointer capture')} onkeydown={(event) => key(event, 'view-hat', true, 'hat', direction.value)} onkeyup={(event) => key(event, 'view-hat', false, 'hat', direction.value)} onblur={() => { recorder.release(-10 - direction.value, 'Control lost focus'); refresh(); }}>{direction.arrow}</button>
        {/each}
        <span class="hat-centre">{Number(snapshot.values['view-hat'] ?? -1) < 0 ? '•' : '●'}</span>
      </div><span class="control-hint">release to neutral</span></div>
    </section>
  </div>
  <div class="feedback-strip"><span class="feedback-dot"></span><span>Engine status</span><strong>LOCAL SAMPLE</strong><span class="feedback-label">Feedback fixture · no host connected</span></div>
</div>
