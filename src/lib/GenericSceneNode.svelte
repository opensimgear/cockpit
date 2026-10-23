<script lang="ts">
  import GenericSceneNode from './GenericSceneNode.svelte';
  import type { VisualNode, WidgetSourceAsset } from '../../packages/widget-compiler/src/types.ts';
  import { evaluateExpression, type Scalar } from '../../packages/widget-runtime/src/behavior.ts';
  let { node, values, regions, assets = [] }: { node: VisualNode; values: Record<string,Scalar>; regions: Record<string,string>; assets?: WidgetSourceAsset[] } = $props();
  function value(input: unknown, fallback: Scalar): Scalar { try { return input && typeof input === 'object' ? evaluateExpression(input as Parameters<typeof evaluateExpression>[0],{values,regions}) : typeof input === 'number' || typeof input === 'string' || typeof input === 'boolean' ? input : fallback; } catch { return fallback; } }
  function n(input: unknown, fallback = 0): number { const result = Number(value(input,fallback)); return Number.isFinite(result)?result:fallback; }
  const transform = $derived(node.transform ? `translate(${n(node.transform.translateX)} ${n(node.transform.translateY)}) translate(${n(node.transform.originX)} ${n(node.transform.originY)}) rotate(${n(node.transform.rotate)}) scale(${n(node.transform.scaleX,1)} ${n(node.transform.scaleY,1)}) translate(${-n(node.transform.originX)} ${-n(node.transform.originY)})` : undefined);
  const image = $derived(node.kind==='image'?assets.find(asset=>asset.id===node.assetId):undefined);
</script>
{#if value(node.visible,true)}
  <g {transform} opacity={n(node.opacity,1)} fill={String(value(node.fill,'none'))} stroke={String(value(node.stroke,'none'))} stroke-width={n(node.strokeWidth)} data-source-layer-id={node.id}>
    {#if node.kind==='group'}{#each node.children as child (child.id)}<GenericSceneNode node={child} {values} {regions} {assets}/>{/each}
    {:else if node.kind==='rectangle'}<rect x={n(node.x)} y={n(node.y)} width={Math.max(0,n(node.width))} height={Math.max(0,n(node.height))} rx={n(node.radius)}/>
    {:else if node.kind==='ellipse'}<ellipse cx={n(node.x)+n(node.width)/2} cy={n(node.y)+n(node.height)/2} rx={Math.max(0,n(node.width)/2)} ry={Math.max(0,n(node.height)/2)}/>
    {:else if node.kind==='line'}<line x1={n(node.x)} y1={n(node.y)} x2={n(node.x2)} y2={n(node.y2)}/>
    {:else if node.kind==='path'}<path d={String(value(node.d,''))}/>
    {:else if node.kind==='text'}<text x={n(node.x)} y={n(node.y)} font-family={node.font==='mono'?'OSGPortableMono,monospace':'OSGPortableSans,sans-serif'} font-size={n(node.fontSize,20)} text-anchor={node.align==='center'?'middle':node.align==='right'?'end':'start'}>{String(value(node.text,''))}</text>
    {:else if node.kind==='image' && image}<image href={`data:${image.mime};base64,${image.base64}`} x={n(node.x)} y={n(node.y)} width={n(node.width)} height={n(node.height)}/>{/if}
  </g>
{/if}
<style>
  @font-face{font-family:OSGPortableSans;src:url('../../packages/companion-ui/assets/fonts/osg-sans.ttf') format('truetype');font-weight:400;font-display:block}
  @font-face{font-family:OSGPortableMono;src:url('../../packages/companion-ui/assets/fonts/osg-mono.ttf') format('truetype');font-weight:400;font-display:block}
  text{font-kerning:none;font-variant-ligatures:none}
</style>
