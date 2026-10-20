<script lang="ts">
  import Icon from './Icon.svelte';
  import { notifications, dismissNotification, pauseNotification, resumeNotification } from './notifications';
</script>

<div class="notifications" role="region" aria-label="Notifications">
  {#each $notifications as notification (notification.id)}
    <div class="toast" class:error={notification.kind === 'error'} class:warning={notification.kind === 'warning'} role={notification.kind !== 'info' ? 'alert' : 'status'}
      onpointerenter={() => pauseNotification(notification.id)} onpointerleave={() => resumeNotification(notification.id)}
      onfocusin={() => pauseNotification(notification.id,'focus')}
      onfocusout={event => { if (!(event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))) resumeNotification(notification.id,'focus'); }}>
      <Icon name={notification.kind !== 'info' ? 'alert' : 'check'} size={19} />
      <p>{notification.message}</p>
      <button aria-label="Dismiss notification" onclick={() => dismissNotification(notification.id)}><Icon name="close" size={16} /></button>
      <div class="toast-countdown" aria-hidden="true">{#key notification.timerVersion}<span class="toast-countdown-fill" style:--countdown-start={notification.remainingMs/notification.durationMs} style:animation-duration={`${notification.remainingMs}ms`} style:animation-play-state={notification.paused?'paused':'running'}></span>{/key}</div>
    </div>
  {/each}
</div>

<style>
  .notifications{position:fixed;right:24px;bottom:24px;z-index:10000;display:flex;flex-direction:column;gap:10px;width:min(400px,calc(100vw - 32px));pointer-events:none}
  .toast{--toast-accent:#64caba;position:relative;right:auto;bottom:auto;left:auto;max-width:none;pointer-events:auto;display:flex;align-items:flex-start;gap:12px;padding:16px;border:1px solid var(--line,#344452);border-left:3px solid var(--toast-accent);border-radius:9px;background:var(--surface-raised,#14212c);color:var(--text,#e1eaf0);box-shadow:0 8px 30px #0005;animation:appear .15s ease-out;overflow:hidden}
  .toast.warning{--toast-accent:#e9b75c}.toast.error{--toast-accent:#ed8383}.toast p{flex:1;margin:0;font-size:13px;line-height:1.5;overflow-wrap:anywhere}.toast button{display:flex;padding:2px;background:transparent;color:inherit;border:0;cursor:pointer;border-radius:3px}.toast button:focus-visible{outline:2px solid #64caba;outline-offset:3px}
  .toast-countdown{position:absolute;bottom:0;left:0;right:0;height:2px;background:#ffffff14;pointer-events:none}.toast-countdown-fill{display:block;width:100%;height:100%;background:var(--toast-accent);transform-origin:left;animation-name:countdown;animation-timing-function:linear;animation-fill-mode:forwards}
  @keyframes countdown{from{transform:scaleX(var(--countdown-start))}to{transform:scaleX(0)}}
  @keyframes appear{from{opacity:0;transform:translateY(8px)}to{opacity:1;transform:translateY(0)}}
  @media(prefers-reduced-motion:reduce){.toast{animation:none}}@media(max-width:600px){.notifications{right:16px;bottom:16px}}
</style>
