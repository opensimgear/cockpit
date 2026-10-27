<script lang="ts">
  let {available}:{available:boolean}=$props();
</script>

{#if available}
  <span class="integration-badge" title="Telemetry integration is available for this game">Integration</span>
{/if}

<style>
  .integration-badge{display:inline-flex;align-items:center;vertical-align:middle;margin-inline-start:8px;padding:3px 7px;border:1px solid color-mix(in srgb,var(--accent) 35%,transparent);border-radius:4px;background:color-mix(in srgb,var(--accent) 10%,transparent);color:var(--accent);font-size:10px;font-weight:600;line-height:1.4;letter-spacing:0;white-space:nowrap}
</style>
