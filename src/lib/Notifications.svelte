<script lang="ts">
  import Icon from './Icon.svelte';
  import { notifications, dismissNotification } from './notifications';
</script>

<div class="notifications" aria-label="Notifications">
  {#each $notifications as notification (notification.id)}
    <div class="toast" class:error={notification.kind === 'error'} class:warning={notification.kind === 'warning'} role={notification.kind !== 'info' ? 'alert' : 'status'}>
      <Icon name={notification.kind !== 'info' ? 'alert' : 'check'} size={19} />
      <p>{notification.message}</p>
      <button aria-label="Dismiss notification" onclick={() => dismissNotification(notification.id)}><Icon name="close" size={16} /></button>
    </div>
  {/each}
</div>

<style>
  .notifications{position:fixed;right:24px;bottom:24px;z-index:10000;display:flex;flex-direction:column;gap:10px;width:min(400px,calc(100vw - 32px));pointer-events:none}
  .toast{position:relative;right:auto;bottom:auto;left:auto;max-width:none;pointer-events:auto;display:flex;align-items:flex-start;gap:12px;padding:16px;border:1px solid var(--line,#344452);border-left:3px solid #64caba;border-radius:9px;background:var(--surface-raised,#14212c);color:var(--text,#e1eaf0);box-shadow:0 8px 30px #0005;animation:appear .15s ease-out}
  .toast.warning{border-left-color:#e9b75c}.toast.error{border-left-color:#ed8383}.toast p{flex:1;margin:0;font-size:13px;line-height:1.5;overflow-wrap:anywhere}.toast button{display:flex;padding:2px;background:transparent;color:inherit;border:0;cursor:pointer;border-radius:3px}.toast button:focus-visible{outline:2px solid #64caba;outline-offset:3px}
  @keyframes appear{from{opacity:0;transform:translateY(8px)}to{opacity:1;transform:translateY(0)}}
  @media(prefers-reduced-motion:reduce){.toast{animation:none}}@media(max-width:600px){.notifications{right:16px;bottom:16px}}
</style>
