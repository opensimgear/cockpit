<script lang="ts">
  import GenericSceneNode from './GenericSceneNode.svelte';
  import type { WidgetSource } from '../../packages/widget-compiler/src/types.ts';
  import { resolvedParameters } from '../../packages/widget-compiler/src/source.ts';
  import { lowerWidgetDesign } from '../../packages/widget-compiler/src/design.ts';
  import { createBehaviorState, type Scalar } from '../../packages/widget-runtime/src/behavior.ts';
  let { source, parameters = {}, fit = 'stretch' }: { source: WidgetSource; parameters?: Record<string, Scalar>; fit?: 'stretch' | 'contain' } = $props();
  const presentation=$derived(lowerWidgetDesign(source,parameters));
  const state = $derived(createBehaviorState(presentation.behavior));
  const values = $derived({ ...resolvedParameters(source, parameters), ...state.values });
</script>
<svg viewBox={`0 0 ${source.size.width} ${source.size.height}`} preserveAspectRatio={fit === 'contain' ? 'xMidYMid meet' : 'none'} aria-hidden="true" data-source-preview="true">
  {#each presentation.scene as node (node.id)}<GenericSceneNode {node} {values} regions={state.regions} assets={source.assets}/>{/each}
</svg>
<style>svg{display:block;width:100%;height:100%;overflow:hidden;background:transparent}</style>
