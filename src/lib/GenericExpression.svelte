<script lang="ts">
  import type { Expression, Scalar } from '../../packages/widget-runtime/src/behavior.ts';
  import Icon from './Icon.svelte';
  import GenericExpression from './GenericExpression.svelte';
  let { value, references = [], regions = [], onchange, label = 'Expression', depth = 0 }: {
    value: Expression; references?: string[]; regions?: string[]; onchange: (value: Expression) => void; label?: string; depth?: number;
  } = $props();
  const mode = $derived('literal' in value ? 'literal' : 'ref' in value ? 'ref' : 'state' in value ? 'state' : value.op);
  const scalarType = $derived('literal' in value ? typeof value.literal : 'boolean');
  function changeMode(kind: string) {
    const literal: Expression = { literal: true };
    if (kind === 'literal') onchange(literal);
    else if (kind === 'ref') onchange({ ref: references[0] ?? '' });
    else if (kind === 'state') onchange({ state: regions[0] ?? '' });
    else if (kind === 'not') onchange({ op: 'not', value: literal });
    else if (kind === 'and' || kind === 'or') onchange({ op: kind, values: [literal, literal] });
    else if (kind === 'clamp') onchange({ op: 'clamp', value: { literal: 0 }, min: { literal: 0 }, max: { literal: 1 } });
    else if (kind === 'if') onchange({ op: 'if', condition: literal, then: { literal: 1 }, else: { literal: 0 } });
    else onchange({ op: kind as 'eq', left: { literal: 0 }, right: { literal: 0 } });
  }
  function literalType(type: string) { onchange({ literal: type === 'number' ? 0 : type === 'boolean' ? true : '' }); }
  function patch(key: string, expression: Expression) { onchange({ ...value, [key]: expression } as Expression); }
</script>

<div class="expression" class:nested={depth > 0}>
  <label>{label}<select aria-label={label + ' operator'} value={mode} onchange={e => changeMode(e.currentTarget.value)}>
    <option value="literal">Value</option><option value="ref">Signal or parameter</option><option value="state">Region state</option>
    {#if depth < 6}{#each ['not','and','or','eq','ne','lt','lte','gt','gte','add','sub','mul','div','clamp','if'] as op}<option value={op}>{({ not:'Not',and:'All',or:'Any',eq:'Equals',ne:'Differs',lt:'Less than',lte:'At most',gt:'Greater than',gte:'At least',add:'Add',sub:'Subtract',mul:'Multiply',div:'Divide',clamp:'Clamp',if:'If / then / else' } as Record<string,string>)[op]}</option>{/each}{/if}
  </select></label>
  {#if 'literal' in value}
    <div class="literal"><select aria-label={label + ' value type'} value={scalarType} onchange={e => literalType(e.currentTarget.value)}><option value="boolean">Boolean</option><option value="number">Number</option><option value="string">Text</option></select>
      {#if typeof value.literal === 'boolean'}<select aria-label={label + ' value'} value={String(value.literal)} onchange={e => onchange({literal:e.currentTarget.value === 'true'})}><option value="true">True</option><option value="false">False</option></select>
      {:else if typeof value.literal === 'number'}<input aria-label={label + ' value'} type="number" value={value.literal} onchange={e => onchange({literal:e.currentTarget.valueAsNumber})}/>
      {:else}<input aria-label={label + ' value'} value={value.literal} onchange={e => onchange({literal:e.currentTarget.value})}/>{/if}
    </div>
  {:else if 'ref' in value}<select aria-label={label + ' reference'} value={value.ref} onchange={e => onchange({ref:e.currentTarget.value})}>{#each references as ref}<option value={ref}>{ref}</option>{/each}</select>
  {:else if 'state' in value}<select aria-label={label + ' region'} value={value.state} onchange={e => onchange({state:e.currentTarget.value})}>{#each regions as region}<option value={region}>{region}</option>{/each}</select>
  {:else if 'value' in value}
    <GenericExpression value={value.value} {references} {regions} depth={depth+1} label="Operand" onchange={(v: Expression) => patch('value',v)}/>
    {#if value.op === 'clamp'}<GenericExpression value={value.min} {references} {regions} depth={depth+1} label="Minimum" onchange={(v: Expression) => patch('min',v)}/><GenericExpression value={value.max} {references} {regions} depth={depth+1} label="Maximum" onchange={(v: Expression) => patch('max',v)}/>{/if}
  {:else if 'values' in value}
    {#each value.values as item,i}<div class="operand"><GenericExpression value={item} {references} {regions} depth={depth+1} label={`Operand ${i+1}`} onchange={(v: Expression) => { if('values' in value) onchange({...value,values:value.values.map((x,j)=>i===j?v:x)}); }}/><button title="Remove operand" aria-label="Remove operand" disabled={value.values.length<=2} onclick={() => {if('values' in value)onchange({...value,values:value.values.filter((_,j)=>j!==i)});}}><Icon name="trash" size={14}/></button></div>{/each}
    <button onclick={() => {if('values' in value)onchange({...value,values:[...value.values,{literal:true}]});}}><Icon name="plus" size={14}/>Add operand</button>
  {:else if 'left' in value}<GenericExpression value={value.left} {references} {regions} depth={depth+1} label="Left" onchange={(v: Expression) => patch('left',v)}/><GenericExpression value={value.right} {references} {regions} depth={depth+1} label="Right" onchange={(v: Expression) => patch('right',v)}/>
  {:else if 'condition' in value}<GenericExpression value={value.condition} {references} {regions} depth={depth+1} label="Condition" onchange={(v: Expression) => patch('condition',v)}/><GenericExpression value={value.then} {references} {regions} depth={depth+1} label="Then" onchange={(v: Expression) => patch('then',v)}/><GenericExpression value={value.else} {references} {regions} depth={depth+1} label="Else" onchange={(v: Expression) => patch('else',v)}/>{/if}
</div>

<style>
  .expression{min-width:0;font-size:var(--ui-text-size);line-height:var(--ui-line-height)}.nested{margin:6px 0 6px 6px;padding-left:8px;border-left:2px solid #36505c}label{display:grid;gap:4px;color:#b6cbd0;margin:5px 0}.literal{display:grid;grid-template-columns:90px minmax(0,1fr);gap:5px}select,input{min-width:0;width:100%;box-sizing:border-box;font:inherit;color:#e0eef0;background:#14232d;border:1px solid #3b5360;border-radius:4px;padding:6px}.operand{display:flex;gap:4px;align-items:start}.operand>:first-child{flex:1}button{display:inline-flex;align-items:center;gap:5px;color:#c5dbde;background:#243945;border:1px solid #3b5360;border-radius:4px;padding:5px;font:inherit;font-size:var(--ui-action-size);cursor:pointer}button:disabled{opacity:.4}button:focus-visible,input:focus-visible,select:focus-visible{outline:2px solid #83d9c1;outline-offset:2px}
</style>
