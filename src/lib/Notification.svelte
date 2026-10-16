<script lang="ts">
  import { notify } from './notifications';
  let { message, kind = 'info' }: { message: string | null | undefined; kind?: 'info' | 'warning' | 'error' } = $props();
  $effect(() => { if (message) notify(message, kind); });
</script>
