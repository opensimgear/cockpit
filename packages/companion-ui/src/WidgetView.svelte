<script lang="ts">
  import type { WidgetDefinition, WidgetLayer, WidgetSample, WidgetState } from '@opensimgear/companion-designer';
  import { safeAssetUrl } from './presentation.ts';
  import { radialGaugePath, widgetGaugeFraction, widgetIndicatorActive, widgetLayerText } from './widget-presentation.ts';
  let { definition, layers, state = 'normal', samples = {}, assets = {}, now = 0 }: {
    definition: WidgetDefinition; layers: WidgetLayer[]; state?: WidgetState;
    samples?: Record<string, WidgetSample | undefined>; assets?: Record<string, string>; now?: number;
  } = $props();
  const uid = $props.id();
  const x = (layer: WidgetLayer) => layer.bounds.x * definition.size.width;
  const y = (layer: WidgetLayer) => layer.bounds.y * definition.size.height;
  const width = (layer: WidgetLayer) => layer.bounds.width * definition.size.width;
  const height = (layer: WidgetLayer) => layer.bounds.height * definition.size.height;
</script>

<svg class="widget-artwork" viewBox={`0 0 ${definition.size.width} ${definition.size.height}`} preserveAspectRatio="none" role="img" aria-label={`${definition.title}: ${state}`} data-widget-state={state}>
  <defs>{#each layers as layer}<clipPath id={`${uid}-${layer.id}`}><rect x={x(layer)} y={y(layer)} width={width(layer)} height={height(layer)} /></clipPath>{/each}</defs>
  {#each layers as layer (layer.id)}
    {#if layer.visible !== false}
      <g opacity={Number(layer.opacity ?? 1)} clip-path={`url(#${uid}-${layer.id})`} data-layer-id={layer.id}>
        {#if layer.kind === 'rectangle'}
          <rect x={x(layer)} y={y(layer)} width={width(layer)} height={height(layer)} rx={Number(layer.radius ?? 0)} fill={String(layer.fill ?? 'none')} stroke={String(layer.stroke ?? 'none')} stroke-width={Number(layer.strokeWidth ?? 0)} />
        {:else if layer.kind === 'ellipse'}
          <ellipse cx={x(layer) + width(layer) / 2} cy={y(layer) + height(layer) / 2} rx={width(layer) / 2} ry={height(layer) / 2} fill={String(layer.fill ?? 'none')} stroke={String(layer.stroke ?? 'none')} stroke-width={Number(layer.strokeWidth ?? 0)} />
        {:else if layer.kind === 'line'}
          <line x1={x(layer)} y1={y(layer) + height(layer) / 2} x2={x(layer) + width(layer)} y2={y(layer) + height(layer) / 2} stroke={String(layer.stroke ?? layer.fill ?? '#ffffff')} stroke-width={Number(layer.strokeWidth ?? 1)} />
        {:else if layer.kind === 'text'}
          <text x={x(layer) + (layer.align === 'center' ? width(layer) / 2 : layer.align === 'right' ? width(layer) : 0)} y={y(layer) + height(layer) / 2} dominant-baseline="central" text-anchor={layer.align === 'center' ? 'middle' : layer.align === 'right' ? 'end' : 'start'} font-family={layer.font === 'mono' ? 'ui-monospace, monospace' : 'Arial, sans-serif'} font-size={layer.fontSize} fill={String(layer.fill ?? '#ffffff')}>{widgetLayerText(layer, samples, now)}</text>
        {:else if layer.kind === 'image'}
          {#if safeAssetUrl(assets[String(layer.assetId)])}<image href={safeAssetUrl(assets[String(layer.assetId)])!} x={x(layer)} y={y(layer)} width={width(layer)} height={height(layer)} preserveAspectRatio="none" />{/if}
        {:else if layer.kind === 'indicator'}
          <ellipse cx={x(layer) + width(layer) / 2} cy={y(layer) + height(layer) / 2} rx={width(layer) / 2} ry={height(layer) / 2} fill={widgetIndicatorActive(layer, samples, now) ? String(layer.fill ?? '#63dfd0') : '#42515a'} />
        {:else if layer.kind === 'gauge'}
          {@const fraction = widgetGaugeFraction(layer, samples, now)}
          {#if layer.orientation === 'radial'}
            <path d={radialGaugePath(x(layer), y(layer), width(layer), height(layer), 1)} fill="none" stroke={String(layer.stroke ?? '#30434d')} stroke-width={Number(layer.strokeWidth ?? 6)} stroke-linecap="round" />
            {#if fraction > 0}<path d={radialGaugePath(x(layer), y(layer), width(layer), height(layer), fraction)} fill="none" stroke={String(layer.fill ?? '#63dfd0')} stroke-width={Number(layer.strokeWidth ?? 6)} stroke-linecap="round" />{/if}
          {:else}
            <rect x={x(layer)} y={y(layer)} width={width(layer)} height={height(layer)} fill={String(layer.stroke ?? '#30434d')} />
            <rect x={x(layer)} y={y(layer) + (layer.orientation === 'vertical' ? height(layer) * (1 - fraction) : 0)} width={width(layer) * (layer.orientation === 'horizontal' ? fraction : 1)} height={height(layer) * (layer.orientation === 'vertical' ? fraction : 1)} fill={String(layer.fill ?? '#63dfd0')} />
          {/if}
        {/if}
      </g>
    {/if}
  {/each}
</svg>

<style>
  .widget-artwork { display: block; width: 100%; height: 100%; overflow: hidden; pointer-events: none; }
</style>
