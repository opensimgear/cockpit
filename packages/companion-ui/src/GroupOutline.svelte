<script lang="ts">
  import { widgetTextLayout } from './widget-fonts.ts';
  let { label, width, height }: { label: string; width: number; height: number } = $props();
  const text = $derived(widgetTextLayout(label, 'sans', 9, 'left', width));
</script>

<svg class="group-artwork" width="100%" height="100%" aria-hidden="true">
  <rect x="-2" y="-2" width={width + 4} height={height + 4} fill="none" stroke="#70978c" stroke-opacity={1 / 3} stroke-dasharray="3 3" />
  <rect x="7" y="-13" width={text.width + 8} height="13" fill="#0c1720" />
  <text y={-6.5 + text.baseline} font-family="OSGPortableSans, sans-serif" font-size="9" fill="#9cb4aa">{#each text.glyphs as glyph}<tspan x={11 + glyph.x}>{glyph.character}</tspan>{/each}</text>
</svg>

<style>
  @font-face { font-family: OSGPortableSans; src: url('../assets/fonts/osg-sans.ttf') format('truetype'); font-style: normal; font-weight: 400; font-display: block; }
  .group-artwork{position:absolute;inset:0;overflow:visible;pointer-events:none;font-kerning:none;font-variant-ligatures:none}
</style>
