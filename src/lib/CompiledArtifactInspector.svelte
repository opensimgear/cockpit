<script lang="ts">
  import type { CompiledWidgetBundle, WidgetCapabilities } from '../../packages/widget-runtime/src/artifact.ts';
  let { bundle, capabilities }: { bundle: CompiledWidgetBundle; capabilities: WidgetCapabilities } = $props();
  const artifact=$derived(bundle.artifact);
  const identities=$derived([
    ['Artifact hash',bundle.artifactHash],['Source hash',artifact.sourceHash],
    ['Parameter hash',artifact.parameterHash],['Behavior hash',artifact.behaviorHash],
    ['Profile hash',artifact.capabilityHash],['Compiler',artifact.compiler],['Rasterizer',artifact.rasterizer],
  ]);
</script>

<section aria-label="Compiled artifact inspector">
  <h3>Compiled artifact</h3>
  <p>{artifact.width} × {artifact.height} · {artifact.pixelFormat} · {artifact.tileSize} px tiles</p>
  <details open><summary>Build identities</summary><dl>{#each identities as [label,value]}<div><dt>{label}</dt><dd>{value}</dd></div>{/each}</dl></details>
  <details><summary>Target capabilities · {capabilities.id}</summary><dl>
    <div><dt>Profile</dt><dd>{capabilities.id} · v{capabilities.version}</dd></div>
    <div><dt>Raster formats</dt><dd>{capabilities.rasterFormats.join(', ')}</dd></div>
    <div><dt>Maximum dimensions</dt><dd>{capabilities.maxWidth} × {capabilities.maxHeight}</dd></div>
    <div><dt>Maximum frames</dt><dd>{capabilities.maxFrames.toLocaleString()}</dd></div>
    <div><dt>Maximum asset bytes</dt><dd>{capabilities.maxAssetBytes.toLocaleString()}</dd></div>
    <div><dt>Maximum transfer bytes</dt><dd>{capabilities.maxTotalBytes.toLocaleString()}</dd></div>
    <div><dt>Maximum resident bytes</dt><dd>{capabilities.maxResidentBytes.toLocaleString()}</dd></div>
    <div><dt>Motion supported</dt><dd>{capabilities.motion?'Yes':'No'}</dd></div>
    <div><dt>Audio supported</dt><dd>{capabilities.audio?'Yes':'No'}</dd></div>
    <div><dt>Audio formats</dt><dd>{capabilities.audioFormats.join(', ')||'None'}</dd></div>
    <div><dt>Compiled motion clips</dt><dd>{artifact.motions.length}</dd></div>
    <div><dt>Compiled sound cues</dt><dd>{artifact.sounds.length}</dd></div>
  </dl></details>
  <details><summary>Sampled visual domains · {artifact.domains.length}</summary>
    {#if !artifact.domains.length}<p>One static presentation per configured state region.</p>{/if}
    {#each artifact.domains as domain}<article aria-label={`Compiled domain ${domain.signal}`}><h4>{domain.signal}</h4><dl>
      <div><dt>Samples</dt><dd>{domain.values.length}</dd></div>
      <div><dt>{domain.numeric?'Range':'Values'}</dt><dd>{domain.numeric?`${domain.values[0]} – ${domain.values.at(-1)}`:domain.values.map(String).join(', ')}</dd></div>
      <div><dt>Maximum error (px)</dt><dd>{domain.maxErrorPixels}</dd></div>
      <div><dt>Smoothing (ms)</dt><dd>{domain.smoothingMs}</dd></div>
    </dl></article>{/each}
  </details>
  <details open><summary>Resource admission</summary><dl>{#each Object.entries(artifact.resources) as [key,value]}<div><dt>{key}</dt><dd>{value.toLocaleString()}</dd></div>{/each}</dl></details>
  {#each artifact.fallbacks as fallback}<p class="fallback">{fallback.feature}: {fallback.reason}</p>{/each}
</section>

<style>
  section{min-width:0}h3{font-size:var(--ui-heading-size);margin:16px 0 9px}h4{font-size:var(--ui-heading-size);margin:0 0 6px}p{font-size:var(--ui-text-size);line-height:var(--ui-line-height);color:#a2bec9}details{border-top:1px solid #2c4551;margin:10px 0;padding:9px 0}summary{cursor:pointer;color:#b2d4d6;font-size:12px}dl{font-size:var(--ui-text-size);margin:10px 0}dl>div{display:grid;grid-template-columns:minmax(90px,1fr) minmax(0,1.4fr);gap:10px;padding:5px 0;border-bottom:1px solid #263d49}dt{color:#94b1bc;overflow-wrap:anywhere}dd{margin:0;overflow-wrap:anywhere;text-align:right;font-variant-numeric:tabular-nums}article{padding:8px 10px;margin:10px 0;background:#10202b;border:1px solid #304b58;border-radius:5px}.fallback{background:#50452a;padding:7px;color:#e6cf92}
</style>
