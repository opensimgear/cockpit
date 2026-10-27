<script lang="ts">
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';

  let { message, kind = 'info', id }: { message: string; kind?: 'info'|'warning'|'error'; id?: string } = $props();
  const uid = $props.id();
  const tooltipId = $derived(id ?? `${uid}-message`);
  const label = $derived(`${kind[0].toUpperCase()}${kind.slice(1)}: ${message}`);
  let trigger = $state<HTMLSpanElement>();
  let tooltip = $state<HTMLSpanElement>();
  let hovered = $state(false), focused = $state(false), dismissed = $state(false);
  const visible = $derived((hovered || focused) && !dismissed);
  let position = $state({ left: 0, top: 0, above: false });

  function resetDismissal() { if (!hovered && !focused) dismissed = false; }
  function dismiss(event: KeyboardEvent) {
    if (event.key !== 'Escape' || !visible) return;
    dismissed = true;
    event.preventDefault();
    event.stopPropagation();
  }
  function placeTooltip() {
    if (!trigger || !tooltip) return;
    const rect = trigger.getBoundingClientRect();
    const box = tooltip.getBoundingClientRect();
    const roomBelow = window.innerHeight - rect.bottom - 14;
    const roomAbove = rect.top - 14;
    const above = box.height > roomBelow && roomAbove > roomBelow;
    const top = above ? rect.top - box.height - 6 : rect.bottom + 6;
    position = { left: Math.max(8, Math.min(rect.left, window.innerWidth - box.width - 8)),
      top: Math.max(8, Math.min(top, window.innerHeight - box.height - 8)), above };
  }
  $effect(() => {
    void message;
    if (!visible) return;
    let cancelled = false;
    void tick().then(() => { if (!cancelled) placeTooltip(); });
    window.addEventListener('resize', placeTooltip);
    window.addEventListener('scroll', placeTooltip, true);
    return () => {
      cancelled = true;
      window.removeEventListener('resize', placeTooltip);
      window.removeEventListener('scroll', placeTooltip, true);
    };
  });
</script>

<svelte:window onkeydown={dismiss}/>
<span class="companion-message" class:open={visible} class:warning={kind==='warning'} class:error={kind==='error'} bind:this={trigger} role="group"
  onpointerenter={() => hovered = true} onpointerleave={() => { hovered = false; resetDismissal(); }}>
  <button type="button" aria-label={label} aria-describedby={tooltipId}
    onfocus={() => focused = true} onblur={() => { focused = false; resetDismissal(); }}>
    <Icon name={kind==='warning'?'alert':kind==='error'?'error':'info'} size={17}/>
  </button>
  <span class="companion-tooltip" class:above={position.above} bind:this={tooltip} id={tooltipId} role="tooltip" hidden={!visible}
    style:left={`${position.left}px`} style:top={`${position.top}px`}><span class="tooltip-copy">{message}</span></span>
</span>

<style>
  .companion-message{position:relative;z-index:2;display:inline-flex;flex-shrink:0;vertical-align:middle;color:var(--accent,#64caba)}
  .companion-message.open{z-index:1001}
  .companion-message.warning{color:var(--warning,#e9b75c)}
  .companion-message.error{color:var(--danger,#e67979)}
  button{display:grid;place-items:center;width:24px;height:24px;padding:0;border:0;border-radius:4px;background:transparent;color:inherit;cursor:help}
  button:focus-visible{outline:2px solid currentColor;outline-offset:2px}
  .companion-tooltip{position:fixed;z-index:1000;display:block;box-sizing:border-box;width:max-content;max-width:min(320px,80vw);padding:10px 12px;border:1px solid var(--line,#344452);border-radius:6px;background:var(--surface-raised,#14212c);box-shadow:0 8px 24px #0005;color:var(--text,#e1eaf0);font-size:var(--ui-text-size);font-weight:400;line-height:var(--ui-line-height);text-align:left;white-space:normal;overflow-wrap:anywhere;cursor:text}
  .companion-tooltip[hidden]{display:none}
  .tooltip-copy{display:block;max-height:calc(100dvh - 38px);overflow:auto}
  .companion-tooltip::before{content:'';position:absolute;left:0;right:0;top:-8px;height:8px}
  .companion-tooltip.above::before{top:auto;bottom:-8px}
</style>
