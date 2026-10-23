<script lang="ts">
  import {untrack,tick,onMount} from 'svelte';
  import CanvasViewControls from './CanvasViewControls.svelte';
  import {canvasFit,canvasPan,canvasZoom,type CanvasViewport} from './profile-canvas';
  import {canvasNavigation,canvasWheelNavigation} from './canvas-navigation';
  import {canvasStyling} from './canvas-styling';
  import {createDiagramRouter} from './profile-diagram';
  import Icon from './Icon.svelte';
  import GenericExpression from './GenericExpression.svelte';
  import GenericSceneNode from './GenericSceneNode.svelte';
  import {lowerWidgetDesign,emptyDesign,designLayers,layerProperty,validateWidgetSource,expressionRefs,designValues,setLayerProperty} from '../../packages/widget-compiler/src/index.ts';
  import type {WidgetSource,SignalNode,TransitionEffect} from '../../packages/widget-compiler/src/types.ts';
  import {createBehaviorState,evaluateExpression,type BehaviorState,type Expression} from '../../packages/widget-runtime/src/behavior.ts';
  let {source,mode,onchange,onappearance,live}: {source:WidgetSource;mode:'States'|'Signals';onchange:(source:WidgetSource)=>void;onappearance:(id:string)=>void;live?:BehaviorState}=$props();
  let regionId=$state(untrack(()=>source.behavior.regions.at(-1)?.id??'')),selected=$state(''),selectedTransition=$state(''),wire=$state(''),notice=$state(''),stateName=$state(''),bindingProperty=$state('opacity');
  let view=$state<CanvasViewport>({x:0,y:0,zoom:1}),layoutBusy=$state(false),autoFit=$state(true),sizes=$state<Record<string,{width:number;height:number}>>({}),positions=$state<Record<string,{x:number;y:number}>>({}),loaded=$state('');
  let viewport:HTMLDivElement;
  let pan:{x:number;y:number;view:CanvasViewport}|undefined;
  let drag:{id:string;x:number;y:number;px:number;py:number}|undefined;
  const layoutKey=$derived(`osg-widget-layout-1:${source.id}:${mode}:${regionId}`);
  $effect(()=>{if(loaded!==layoutKey){try{const saved=JSON.parse(localStorage.getItem(layoutKey)??'{}');positions=Object.fromEntries(Object.entries(saved??{}).filter((entry):entry is [string,{x:number;y:number}]=>{const p=entry[1] as {x:number;y:number}|null;return !!p&&Number.isFinite(p.x)&&Number.isFinite(p.y)&&p.x>=0&&p.y>=0&&p.x<=20000&&p.y<=20000;}));}catch{positions={};}loaded=layoutKey;autoFit=true;wire='';notice='';void tick().then(()=>fit());}});
  const region=$derived(source.behavior.regions.find(r=>r.id===regionId));
  const transition=$derived(region?.transitions.find(t=>t.id===selectedTransition));
  const validation=$derived(validateWidgetSource(source));
  const lowered=$derived.by(()=>{try{return lowerWidgetDesign(source);}catch{return undefined;}});
  const initial=$derived.by(()=>{try{return lowered?createBehaviorState(lowered.behavior):undefined;}catch{return undefined;}});
  const values=$derived(designValues(source,{values:{...Object.fromEntries(source.parameters.map(p=>[p.id,p.default])),...(live??initial)?.values},regions:(live??initial)?.regions??{}}));
  const references=$derived([...source.behavior.signals.map(s=>s.id),...(source.design?.nodes.map(n=>n.id)??[])]);
  const graphNode=$derived(source.design?.nodes.find(n=>n.id===selected));
  const owner=$derived(source.design?.owners.find(o=>o.region===regionId));
  const ownedSignal=$derived(source.behavior.signals.find(s=>s.id===owner?.signal));
  const presentation=$derived(selected.startsWith('layer:')?designLayers(source.scene).find(n=>n.id===selected.slice(6)):undefined);
  const cards=$derived(mode==='States'?(region?.states??[]).map((id,i)=>({id,label:id,kind:region?.initial===id?'Initial state':'State',x:80+i*280,y:90,expression:undefined as Expression|undefined})):[
    ...source.behavior.signals.map((s,i)=>({id:s.id,label:s.id,kind:s.source==='feedback'?'From profile · '+s.type:s.source==='input'?'Input · '+s.type:'Local · '+s.type,x:40,y:40+i*125,expression:undefined as Expression|undefined})),
    ...(source.design?.nodes??[]).map((n,i)=>({id:n.id,label:n.label,kind:'Transform · '+n.domain.type,x:350,y:60+i*150,expression:n.expression})),
    ...source.inputs.map((p,i)=>({id:'output:'+p.id,label:p.label,kind:'To profile · '+p.type,x:680,y:50+i*140,expression:{ref:p.signal} as Expression})),
    ...source.behavior.regions.map((r,i)=>({id:'state:'+r.id,label:r.id,kind:'State machine',x:680,y:250+i*140,expression:undefined as Expression|undefined})),
    ...source.behavior.events.map((e,i)=>({id:'event:'+e.id,label:e.id,kind:'Interaction event',x:1000,y:40+i*115,expression:undefined as Expression|undefined})),
    ...designLayers(source.scene).filter(n=>n.kind!=='group').map((n,i)=>({id:'layer:'+n.id,label:n.label??n.id,kind:'Presentation',x:1300,y:40+i*115,expression:undefined as Expression|undefined})),
  ]);
  const wires=$derived(mode==='States'?(region?.transitions??[]).filter(t=>t.to).flatMap(t=>(t.from==='*'?region!.states:[t.from]).map(from=>({id:t.id+from,from,to:t.to!,label:t.event,transition:t.id}))):[
    ...cards.flatMap(card=>card.expression?inputPins(card.expression).flatMap(pin=>{const expr=pinExpression(card.expression!,pin),refs=expressionRefs(expr);return[...refs.refs,...[...refs.regions].map(id=>'state:'+id)].map(ref=>({id:ref+card.id+pin,from:ref,to:card.id,pin,label:'value',transition:''}));}):[]),
    ...source.behavior.regions.flatMap(r=>r.transitions.map(t=>({id:r.id+t.id,from:'event:'+t.event,to:'state:'+r.id,label:t.id,transition:''}))),
    ...(source.design?.owners??[]).map(o=>({id:o.region+o.signal,from:'state:'+o.region,to:o.signal,label:'owns',transition:''})),
    ...designLayers(source.scene).flatMap(n=>[...expressionRefs(n).refs].filter(id=>references.includes(id)).map(id=>({id:id+n.id,from:id,to:'layer:'+n.id,label:'appearance',transition:''}))),
    ...(source.design?.appearances??[]).flatMap(a=>Object.keys(a.layers).flatMap(layer=>Object.keys(a.regions).map(r=>({id:a.id+layer+r,from:'state:'+r,to:'layer:'+layer,label:a.id,transition:''})))),
  ]);
  function inputPins(expression:Expression):string[]{return 'op'in expression?'values'in expression?expression.values.map((_,i)=>'values.'+i):Object.keys(expression).filter(k=>k!=='op'):['value'];}
  function pinExpression(expression:Expression,pin:string):Expression{if(!('op'in expression))return expression;if(pin.startsWith('values.')&&'values'in expression)return expression.values[Number(pin.split('.')[1])];return (expression as unknown as Record<string,Expression>)[pin];}
  function measure(element:HTMLElement){
    const update=()=>{const id=element.dataset.graphNode!,width=element.offsetWidth,height=element.offsetHeight;if(sizes[id]?.width!==width||sizes[id]?.height!==height)sizes={...sizes,[id]:{width,height}};};
    update();const observer=new ResizeObserver(update);observer.observe(element);return{destroy(){observer.disconnect();}};
  }
  const routeNodes=$derived(cards.map(card=>{const position=point(card.id),size=sizes[card.id]??{width:210,height:mode==='States'?225:110};return{id:card.id,position,...size,inputAnchors:Object.fromEntries((card.expression?inputPins(card.expression):['value']).map((pin,i)=>[pin,{x:position.x,y:position.y+70+i*25}])),outputAnchors:{value:{x:position.x+size.width,y:position.y+70}}};}));
  const bindings=$derived(wires.filter(w=>cards.some(c=>c.id===w.from)&&cards.some(c=>c.id===w.to)).map((w,i)=>({id:String(i),from:{nodeId:w.from,portId:'value'},to:{nodeId:w.to,portId:'pin'in w?String(w.pin):'value'},transition:w.transition})));
  const router=createDiagramRouter();
  const routes=$derived(router(routeNodes,bindings,210,110,{style:$canvasStyling.arrowStyle,cornerRadius:$canvasStyling.cornerRadius}));
  const layoutSignature=$derived(JSON.stringify({key:layoutKey,nodes:routeNodes,bindings}));
  function fit(){if(viewport?.clientWidth&&viewport?.clientHeight){view=canvasFit([...routeNodes,...routes.flatMap(route=>route.points.map(position=>({position,width:1,height:1})))],viewport.clientWidth,viewport.clientHeight);autoFit=true;}}
  function zoom(factor:number){autoFit=false;view=canvasZoom(view,factor,{x:viewport.clientWidth/2,y:viewport.clientHeight/2});}
  async function layout(){
    if(layoutBusy)return;layoutBusy=true;notice='';const signature=layoutSignature;
    try{const {layoutProfileWithElk}=await import('./profile-elk-runtime');const result=await layoutProfileWithElk(routeNodes,bindings,210,110);
      if(signature!==layoutSignature){notice='The graph changed while arranging. Run Auto-layout again.';return;}
      positions=Object.fromEntries(result.positions.map(p=>[p.nodeId,{x:p.x,y:p.y}]));saveLayout();await tick();fit();
    }catch(error){notice=error instanceof Error?error.message:'Auto-layout failed. Please try again.';}finally{layoutBusy=false;}
  }
  onMount(()=>{const observer=new ResizeObserver(()=>{if(autoFit)fit();});observer.observe(viewport);return()=>observer.disconnect();});
  $effect(()=>{routeNodes;if(autoFit)untrack(fit);});
  function wheel(event:WheelEvent){event.preventDefault();autoFit=false;const navigation=canvasWheelNavigation($canvasNavigation.wheelAction,event,viewport.clientHeight),rect=viewport.getBoundingClientRect();view=navigation.zoom?canvasZoom(view,navigation.multiplier,{x:event.clientX-rect.left,y:event.clientY-rect.top}):canvasPan(view,{x:-navigation.deltaX,y:-navigation.deltaY});}
  function beginPan(event:PointerEvent){if(event.button!==0||(event.target as HTMLElement).closest('article'))return;viewport.focus();autoFit=false;pan={x:event.clientX,y:event.clientY,view:{...view}};viewport.setPointerCapture(event.pointerId);}
  function movePan(event:PointerEvent){if(pan)view=canvasPan(pan.view,{x:event.clientX-pan.x,y:event.clientY-pan.y});}
  function endPan(){pan=undefined;}
  function navigate(event:KeyboardEvent){
    if(event.key==='Escape'){wire='';return;}
    if(event.target!==viewport)return;
    if(event.key==='+'||event.key==='='){event.preventDefault();zoom(1.2);}else if(event.key==='-'){event.preventDefault();zoom(1/1.2);}else if(event.key==='0'){event.preventDefault();fit();}
    else if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)){event.preventDefault();autoFit=false;view=canvasPan(view,{x:event.key==='ArrowLeft'?40:event.key==='ArrowRight'?-40:0,y:event.key==='ArrowUp'?40:event.key==='ArrowDown'?-40:0});}
  }
  function point(id:string){const card=cards.find(n=>n.id===id);return positions[id]??{x:card?.x??0,y:card?.y??0};}
  function change(fn:(next:WidgetSource)=>void){const next=structuredClone(source);next.design??=emptyDesign();fn(next);onchange(next);}
  function unique(prefix:string){return prefix+'-'+crypto.randomUUID().slice(0,6);}
  function addState(){const name=stateName.trim()||unique('state');if(!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(name)||region?.states.includes(name)){notice='Use a unique state ID with letters, numbers, underscores or hyphens.';return;}change(next=>{const r=next.behavior.regions.find(r=>r.id===regionId)!;r.states.push(name);const o=next.design!.owners.find(o=>o.region===regionId);if(o)o.values[name]=next.behavior.signals.find(s=>s.id===o.signal)!.initial;});selected=name;stateName='';}
  function removeState(){if(!region||region.states.length<2)return;change(next=>{const r=next.behavior.regions.find(r=>r.id===regionId)!;r.states=r.states.filter(s=>s!==selected);const removed=new Set(r.transitions.filter(t=>t.from===selected||t.to===selected).map(t=>t.id));r.transitions=r.transitions.filter(t=>!removed.has(t.id));if(r.initial===selected)r.initial=r.states[0];const o=next.design!.owners.find(o=>o.region===r.id);if(o){delete o.values[selected];next.behavior.signals.find(s=>s.id===o.signal)!.initial=o.values[r.initial];}next.design!.appearances=next.design!.appearances.filter(a=>a.regions[r.id]!==selected);next.design!.effects=next.design!.effects.filter(e=>e.region!==r.id||!removed.has(e.transition));next.motions=next.motions?.filter(m=>m.fromRegions?.[r.id]!==selected&&m.toRegions?.[r.id]!==selected);});selected='';}
  function setOwner(signalId:string){change(next=>{next.design!.owners=next.design!.owners.filter(o=>o.region!==regionId);if(signalId){const signal=next.behavior.signals.find(s=>s.id===signalId)!;next.design!.owners.push({region:regionId,signal:signalId,values:Object.fromEntries(region!.states.map(state=>[state,signal.initial]))});}});}
  function setOwnedValue(state:string,value:boolean|number|string){change(next=>{const o=next.design!.owners.find(o=>o.region===regionId)!;o.values[state]=value;if(region!.initial===state)next.behavior.signals.find(s=>s.id===o.signal)!.initial=value;});}
  function saveLayout(){try{localStorage.setItem(layoutKey,JSON.stringify(positions));}catch{notice='Layout could not be stored on this browser.';}}
  function begin(event:PointerEvent,id:string){if(event.button!==0||(event.target as HTMLElement).closest('button,input,select'))return;selected=id;autoFit=false;const p=point(id);drag={id,x:event.clientX,y:event.clientY,px:p.x,py:p.y};(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);}
  function move(event:PointerEvent){if(drag)positions={...positions,[drag.id]:{x:Math.max(0,drag.px+(event.clientX-drag.x)/view.zoom),y:Math.max(0,drag.py+(event.clientY-drag.y)/view.zoom)}};}
  function end(){if(drag){drag=undefined;saveLayout();}}
  function key(event:KeyboardEvent,id:string){if(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)){event.preventDefault();event.stopPropagation();autoFit=false;const p=point(id);positions={...positions,[id]:{x:Math.max(0,p.x+(event.key==='ArrowLeft'?-20:event.key==='ArrowRight'?20:0)),y:Math.max(0,p.y+(event.key==='ArrowUp'?-20:event.key==='ArrowDown'?20:0))}};saveLayout();}else if(event.key==='Escape')wire='';}
  function connect(target:string,pin='value'){
    if(!wire||target===wire){wire=target;return;}
    if(mode==='States')change(next=>{if(!next.behavior.events.length)next.behavior.events.push({id:'activate'});next.behavior.regions.find(r=>r.id===regionId)!.transitions.push({id:unique('transition'),from:wire,to:target,event:next.behavior.events[0].id});});
    else {
      const node=source.design?.nodes.find(n=>n.id===target);
      if(target.startsWith('layer:')&&references.includes(wire)){const next=structuredClone(source),layer=designLayers(next.scene).find(n=>n.id===target.slice(6))!;setLayerProperty(layer,bindingProperty,{ref:wire});const checked=validateWidgetSource(next);if(!checked.ok){notice=checked.error;wire='';return;}onchange(next);wire='';selected=target;notice='Base presentation binding created.';return;}
      if(!node){notice='Connect a value to a transform or presentation. Configure published outputs in the state ownership inspector.';return;}
      const next=structuredClone(source),destination=next.design!.nodes.find(n=>n.id===target)!;
      if(!('op'in destination.expression))destination.expression={ref:wire};
      else if(pin.startsWith('values.')&&'values'in destination.expression)destination.expression.values[Number(pin.split('.')[1])]={ref:wire};
      else destination.expression={...destination.expression,[pin]:{ref:wire}} as Expression;
      const checked=validateWidgetSource(next);
      if(!checked.ok){notice=checked.error;wire='';return;}onchange(next);
    }
    wire='';notice='Connection created.';
  }
  function appearance(state:string){const found=source.design?.appearances.find(a=>Object.keys(a.regions).length===1&&a.regions[regionId]===state);if(found){onappearance(found.id);return;}const id=unique('appearance');change(next=>next.design!.appearances.push({id,regions:{[regionId]:state},layers:{}}));onappearance(id);}
  function addNode(kind:string){change(next=>{const id=unique('value');let node:SignalNode={id,label:kind,domain:{type:'boolean'},expression:{literal:false}};
    if(kind==='Not')node.expression={op:'not',value:{literal:false}};
    if(kind==='Clamp')node={id,label:kind,domain:{type:'number',min:0,max:1},expression:{op:'clamp',value:{literal:0},min:{literal:0},max:{literal:1}}};
    if(kind==='Map range')node={id,label:kind,domain:{type:'number',min:0,max:100},expression:{op:'mul',left:{op:'clamp',value:{literal:0},min:{literal:0},max:{literal:1}},right:{literal:100}}};
    next.design!.nodes.push(node);selected=id;
  });}
  function patchTransition(patch:Record<string,unknown>){change(next=>Object.assign(next.behavior.regions.find(r=>r.id===regionId)!.transitions.find(t=>t.id===selectedTransition)!,patch));}
  const selectedEffect=$derived(source.design?.effects.find(e=>e.region===regionId&&e.transition===selectedTransition));
  function patchEffect(patch:Partial<TransitionEffect>){change(next=>{let item=next.design!.effects.find(e=>e.region===regionId&&e.transition===selectedTransition);if(!item){item={region:regionId,transition:selectedTransition,durationMs:160,fps:30,properties:[],easing:'ease-in-out',interruption:'reverse'};next.design!.effects.push(item);}Object.assign(item,patch);});}
  function probe(expression:Expression){try{return String(evaluateExpression(expression,{values,regions:(live??initial)?.regions??{}}));}catch{return '—';}}

</script>

<div class="graph-editor">
  <div class="toolbar">
    {#if mode==='States'}<label>Region<select bind:value={regionId}>{#each source.behavior.regions as r}<option value={r.id}>{r.id}</option>{/each}</select></label>
      <button onclick={()=>{const id=unique('region');change(next=>next.behavior.regions.push({id,initial:'ready',states:['ready'],transitions:[]}));regionId=id;}}><Icon name="plus" size={14}/>Region</button>
      <input aria-label="New state ID" placeholder="State ID" bind:value={stateName}/><button disabled={!region} onclick={addState}><Icon name="plus" size={14}/>State</button>
    {:else}<details class="add"><summary>＋ Add</summary>{#each ['Value','Not','Clamp','Map range'] as kind}<button onclick={()=>addNode(kind)}><Icon name="plus" size={14}/>{kind}</button>{/each}</details>{/if}
    <span class="instructions">{wire?'Choose a destination port. Escape cancels.':'Drag headers to arrange. Connect ports by click or drag.'}</span>
    <CanvasViewControls zoom={view.zoom} {layoutBusy} onfit={fit} onlayout={layout} onzoom={zoom}/>

  </div>
  {#if !validation.ok}<p class="diagnostic" role="alert">{validation.error}</p>{/if}{#if notice}<p class="notice" role="status">{notice}</p>{/if}
  {#if layoutBusy}<p class="notice" role="status">Arranging blocks and connections…</p>{/if}
  <p class="canvas-help" id="widget-canvas-help">Drag the background to pan. Mouse wheel {$canvasNavigation.wheelAction==='zoom'?'zooms':'scrolls'}; Ctrl/⌘ + wheel {$canvasNavigation.wheelAction==='zoom'?'scrolls':'zooms'}. Configure navigation and arrow style in Settings. Use +/− to zoom and 0 to fit.</p>
  <div class="body">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Keyboard pan/zoom canvas.) -->
    <div class="viewport" role="application" aria-label={`${mode} canvas`} aria-describedby="widget-canvas-help" tabindex="0" bind:this={viewport} onwheel={wheel} onpointerdown={beginPan} onpointermove={movePan} onpointerup={endPan} onpointercancel={endPan} onlostpointercapture={endPan} onkeydown={navigate}>
      <div class="plane" style:transform={`translate(${view.x}px,${view.y}px) scale(${view.zoom})`}>
        <svg class="wires" width="100%" height="100%" aria-label="Connections"><defs><marker id={`arrow-${mode}`} markerUnits="userSpaceOnUse" markerWidth="10" markerHeight="10" viewBox="0 0 8 8" refX="8" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 Q1,4 0,0 Z" fill="var(--accent)"/></marker></defs>
          {#each routes as route}<path class:active={!!selectedTransition&&selectedTransition===bindings.find(b=>b.id===route.binding.id)?.transition} d={route.path} marker-end={`url(#arrow-${mode})`}/>{/each}
        </svg>
        {#each cards as card (card.id)}{@const p=point(card.id)}<article class:processing={card.kind.startsWith('Transform')} class:chosen={selected===card.id} class:live={mode==='States'&&live?.regions[regionId]===card.id} style:left={`${p.x}px`} style:top={`${p.y}px`} data-graph-node={card.id} use:measure style:min-height={`${card.expression?85+inputPins(card.expression).length*25:110}px`}>
          <header tabindex="0" role="button" aria-label={`Move ${card.label}`} onpointerdown={e=>begin(e,card.id)} onpointermove={move} onpointerup={end} onpointercancel={end} onlostpointercapture={end} onclick={()=>{selected=card.id;selectedTransition='';}} onkeydown={e=>{if(e.key==='Enter')selected=card.id;key(e,card.id);}}><Icon name="grip" size={14}/><strong>{card.label}</strong><small>{card.kind}</small></header>
          {#if mode==='States'&&lowered}<svg class="thumbnail" viewBox={`0 0 ${source.size.width} ${source.size.height}`} aria-hidden="true">{#each lowered.scene as node}<GenericSceneNode {node} {values} regions={{...initial?.regions,[regionId]:card.id}} assets={source.assets}/>{/each}</svg><button class="edit" onclick={()=>appearance(card.id)}><Icon name="pencil" size={12}/>Appearance</button>{:else}<div class="value">{card.id in values?String(values[card.id]):card.expression?probe(card.expression):'Event / state'}</div>{/if}
          {#each mode==='Signals'&&card.expression&&source.design?.nodes.some(n=>n.id===card.id)?inputPins(card.expression):['value'] as pin,i}<button class="port input" data-connection-target={card.id} style:top={`${60+i*25}px`} aria-label={`Connect to ${card.label} ${pin}`} title={`Connect ${pin}`} onclick={()=>connect(card.id,pin)} ondragover={e=>e.preventDefault()} ondrop={e=>{e.preventDefault();connect(card.id,pin);}}><span class="port-dot" aria-hidden="true"></span>{#if i>0}<small>{pin}</small>{/if}</button>{/each}
          <button class="port output" data-connection-source={card.id} class:connection-active={wire===card.id} aria-label={`Connect from ${card.label}`} title="Connect output" draggable="true" ondragstart={e=>{wire=card.id;e.dataTransfer?.setData('text/plain',card.id);}} onclick={()=>wire=card.id}><span class="port-dot" aria-hidden="true"></span></button>
        </article>{/each}
      </div>
  </div><aside>
    {#if mode==='States'&&region}
      <h3>{region.id}</h3><label>Initial state<select value={region.initial} onchange={e=>change(next=>{const r=next.behavior.regions.find(r=>r.id===regionId)!;r.initial=e.currentTarget.value;const owner=next.design?.owners.find(o=>o.region===r.id);if(owner)next.behavior.signals.find(s=>s.id===owner.signal)!.initial=owner.values[r.initial];})}>{#each region.states as state}<option>{state}</option>{/each}</select></label>
      <label>Owns value<select value={owner?.signal??''} onchange={e=>setOwner(e.currentTarget.value)}><option value="">None</option>{#each source.behavior.signals.filter(s=>s.source==='local') as signal}<option value={signal.id}>{signal.id}</option>{/each}</select></label>
      {#if owner&&ownedSignal}<label>To profile<select value={owner.port??''} onchange={e=>change(next=>{const o=next.design!.owners.find(o=>o.region===regionId)!;if(e.currentTarget.value)o.port=e.currentTarget.value;else delete o.port;})}><option value="">None</option>{#each source.inputs.filter(p=>p.signal===owner.signal) as port}<option value={port.id}>{port.label}</option>{/each}</select></label>
      {#each region.states as state}<label>Value · {state}{#if ownedSignal.type==='boolean'}<select value={String(owner.values[state])} onchange={e=>setOwnedValue(state,e.currentTarget.value==='true')}><option>false</option><option>true</option></select>{:else if ownedSignal.type==='number'}<input type="number" value={Number(owner.values[state])} min={ownedSignal.min} max={ownedSignal.max} onchange={e=>setOwnedValue(state,e.currentTarget.valueAsNumber)}/>{:else}<select value={String(owner.values[state])} onchange={e=>setOwnedValue(state,e.currentTarget.value)}>{#each ownedSignal.values as value}<option>{value}</option>{/each}</select>{/if}</label>{/each}{/if}
      <h3>Transitions</h3>{#each region.transitions as t}<button class="transition" class:chosen={selectedTransition===t.id} onclick={()=>{selectedTransition=t.id;selected='';}}><Icon name="arrow" size={14}/><span>{t.from} → {t.to??t.from}<small>{t.event}</small></span></button>{/each}
      {#if selected&&region.states.includes(selected)}<h3>{selected}</h3><button onclick={()=>appearance(selected)}><Icon name="pencil" size={14}/>Edit appearance</button><button disabled={region.states.length<2} onclick={removeState}><Icon name="trash" size={14}/>Delete</button><p>Deleting a state removes its transitions, appearances and effects. Remaining expression references are checked before compilation.</p>{/if}
      {#if transition}<h3>Transition</h3><label>Event<select value={transition.event} onchange={e=>patchTransition({event:e.currentTarget.value})}>{#each source.behavior.events as event}<option value={event.id}>{event.id}</option>{/each}</select></label><label>Destination<select value={transition.to??transition.from} onchange={e=>patchTransition({to:e.currentTarget.value})}>{#each region.states as state}<option>{state}</option>{/each}</select></label><label>Priority<input type="number" value={transition.priority??0} onchange={e=>patchTransition({priority:e.currentTarget.valueAsNumber})}/></label>
        <GenericExpression label="Guard" value={transition.guard??{literal:true}} {references} regions={source.behavior.regions.map(r=>r.id)} onchange={guard=>patchTransition({guard})}/>
        <details><summary>Motion and sound</summary><label>Duration (ms)<input type="number" min="1" max="60000" value={selectedEffect?.durationMs??160} onchange={e=>patchEffect({durationMs:e.currentTarget.valueAsNumber})}/></label><label>Frames / second<input type="number" min="1" max="60" value={selectedEffect?.fps??30} onchange={e=>patchEffect({fps:e.currentTarget.valueAsNumber})}/></label><label>Easing<select value={selectedEffect?.easing??'ease-in-out'} onchange={e=>patchEffect({easing:e.currentTarget.value as TransitionEffect['easing']})}>{#each ['linear','ease-in','ease-out','ease-in-out'] as value}<option>{value}</option>{/each}</select></label><label>Interruption<select value={selectedEffect?.interruption??'reverse'} onchange={e=>patchEffect({interruption:e.currentTarget.value as TransitionEffect['interruption']})}>{#each ['reverse','restart','snap'] as value}<option>{value}</option>{/each}</select></label>
        <p>Animate numeric properties between the state appearances.</p>{#each designLayers(source.scene) as node}<details><summary>{node.label??node.id}</summary>{#each ['x','y','opacity','transform.translateX','transform.translateY','transform.scaleX','transform.scaleY'] as property}{#if property.startsWith('transform.')||property in node}<label class="check"><input type="checkbox" checked={selectedEffect?.properties.some(p=>p.node===node.id&&p.property===property)??false} onchange={e=>patchEffect({properties:e.currentTarget.checked?[...(selectedEffect?.properties??[]),{node:node.id,property}]:(selectedEffect?.properties??[]).filter(p=>p.node!==node.id||p.property!==property)})}/>{property}</label>{/if}{/each}</details>{/each}
        <label>Sound cue<select value={selectedEffect?.cue??''} onchange={e=>patchEffect({cue:e.currentTarget.value||undefined})}><option value="">Silent</option>{#each source.sounds??[] as sound}<option value={sound.cue}>{sound.cue}</option>{/each}</select></label><button onclick={()=>change(next=>next.design!.effects=next.design!.effects.filter(e=>e.region!==regionId||e.transition!==selectedTransition))}><Icon name="trash" size={14}/>Remove effects</button></details>
        <button onclick={()=>change(next=>{next.behavior.regions.find(r=>r.id===regionId)!.transitions=region.transitions.filter(t=>t.id!==selectedTransition);next.design!.effects=next.design!.effects.filter(e=>e.region!==regionId||e.transition!==selectedTransition);selectedTransition='';})}><Icon name="trash" size={14}/>Delete</button>
      {/if}
    {:else if presentation}<h3>{presentation.label??presentation.id}</h3><label>Base property<select bind:value={bindingProperty}>{#each ['opacity','visible','x','y','width','height','fill','text','rotation','transform.translateX','transform.translateY','transform.scaleX','transform.scaleY'].filter(p=>p in presentation||p.startsWith('transform.')||p==='opacity'||p==='visible') as property}<option>{property}</option>{/each}</select></label><GenericExpression label="Binding" value={typeof layerProperty(presentation,bindingProperty)==='object'?layerProperty(presentation,bindingProperty) as Expression:{literal:layerProperty(presentation,bindingProperty) as boolean|number|string}} {references} regions={source.behavior.regions.map(r=>r.id)} onchange={expression=>change(next=>setLayerProperty(designLayers(next.scene).find(n=>n.id===presentation.id)!,bindingProperty,expression))}/><p>Wires to this presentation update the selected base property. State appearances can override it.</p>
    {:else if graphNode}<h3>{graphNode.label}</h3><label>Label<input value={graphNode.label} onchange={e=>change(next=>next.design!.nodes.find(n=>n.id===selected)!.label=e.currentTarget.value)}/></label><label>Output type<select value={graphNode.domain.type} onchange={e=>change(next=>next.design!.nodes.find(n=>n.id===selected)!.domain=e.currentTarget.value==='number'?{type:'number',min:0,max:100}:e.currentTarget.value==='enum'?{type:'enum',values:['off','on']}:{type:'boolean'})}><option>boolean</option><option>number</option><option>enum</option></select></label>
      {#if graphNode.domain.type==='number'}{#each ['min','max'] as key}<label>{key}<input type="number" value={graphNode.domain[key as 'min'|'max']} onchange={e=>change(next=>Object.assign(next.design!.nodes.find(n=>n.id===selected)!.domain,{[key]:e.currentTarget.valueAsNumber}))}/></label>{/each}{:else if graphNode.domain.type==='enum'}<label>Values<input value={graphNode.domain.values.join(',')} onchange={e=>change(next=>next.design!.nodes.find(n=>n.id===selected)!.domain={type:'enum',values:e.currentTarget.value.split(',').map(v=>v.trim())})}/></label>{/if}
      <GenericExpression value={graphNode.expression} references={references.filter(id=>id!==selected)} regions={source.behavior.regions.map(r=>r.id)} onchange={expression=>change(next=>next.design!.nodes.find(n=>n.id===selected)!.expression=expression)}/>
      <p>Expressions are typed wires. Select a reference to connect a named input. Guards, actions and layer bindings can reference this value.</p><button onclick={()=>change(next=>next.design!.nodes=next.design!.nodes.filter(n=>n.id!==selected))}><Icon name="trash" size={14}/>Delete</button>
    {:else}<h3>{selected||'Signal flow'}</h3><p>Select a transform to edit its inputs and output domain. Data defines profile ports; Behavior defines event connections and actions.</p><p>From profile is feedback. To profile is the requested command. State machines own retained values.</p>{/if}
  </aside></div>
</div>
<style>
  .graph-editor{display:flex;flex-direction:column;height:100%;min-height:580px;color:var(--text)}.toolbar{display:flex;gap:10px;align-items:center;padding:14px;flex-wrap:wrap;border-bottom:1px solid var(--line);background:var(--surface)}.toolbar label{display:flex;align-items:center;gap:7px}.instructions{flex:1;color:var(--muted);font-size:12px}.body{display:grid;grid-template-columns:minmax(0,1fr) 280px;flex:1;min-height:0}.viewport{position:relative;min-height:480px;height:65vh;max-height:850px;overflow:hidden;touch-action:none;background-color:var(--bg);background-image:radial-gradient(var(--line) 1px,transparent 1px);background-size:20px 20px}.viewport:focus-visible{outline:2px solid var(--accent);outline-offset:-3px}.canvas-help{padding:10px 14px;margin:0;border-bottom:1px solid var(--line)}.plane{position:absolute;left:0;top:0;width:1px;height:1px;transform-origin:top left}.wires{position:absolute;inset:0;overflow:visible;pointer-events:none}.wires>path{fill:none;stroke:var(--accent);stroke-width:2;opacity:.65}.wires>path.active{stroke-width:3;opacity:1}article{position:absolute;width:210px;box-sizing:border-box;border:1px solid var(--line);border-radius:9px;background:var(--surface);box-shadow:0 5px 14px #0002}article.chosen{outline:2px solid var(--accent)}article.live{box-shadow:0 0 0 4px #a0d9ab33}header{display:flex;gap:5px;flex-wrap:wrap;padding:10px;background:var(--surface-raised);border-radius:8px 8px 0 0;cursor:grab;touch-action:none}header strong{font-size:13px;overflow-wrap:anywhere}header small{width:100%;color:var(--muted);font-size:11px;padding-left:20px}.thumbnail{display:block;width:180px;height:100px;margin:8px auto}.edit{margin:0 10px 10px}.value{padding:16px;color:var(--accent);font-family:monospace}.port{position:absolute;top:60px;background:transparent;border:0;width:20px;height:20px;padding:0;cursor:crosshair}.port-dot{position:absolute;left:50%;top:50%;transform:translate(-50%,-50%);width:8px;height:8px;box-sizing:border-box;border:1px solid var(--accent);border-radius:50%;background:var(--surface);pointer-events:none;z-index:3}.port:hover:not(:disabled)>.port-dot,.port.connection-active>.port-dot{background:var(--accent)}article.processing{border-color:#b794d2}.processing .port-dot{border-color:#c6a0e5}.processing .port:hover:not(:disabled)>.port-dot,.processing .port.connection-active>.port-dot{background:#c6a0e5}.port small{position:absolute;left:21px;white-space:nowrap}.input{left:-11px}.output{right:-11px}aside{overflow:auto;border-left:1px solid var(--line);padding:15px;background:var(--surface);max-height:72vh}h3{font-size:14px;margin:16px 0 10px}p{font-size:12px;color:var(--muted);line-height:1.5}label{display:grid;gap:5px;margin:10px 0;font-size:12px}button,select,input{font:inherit;color:var(--text);background:var(--input-bg);border:1px solid var(--line);border-radius:5px;padding:7px;min-width:0}button{display:inline-flex;gap:6px;align-items:center;cursor:pointer}button:hover{border-color:var(--accent)}button:disabled{opacity:.4;cursor:default}button:focus-visible,header:focus-visible{outline:2px solid var(--accent)}.transition{display:flex;width:100%;margin:5px 0;text-align:left}.transition.chosen{background:var(--accent-bg);border-color:var(--accent)}.transition small{display:block;color:var(--muted);font-size:11px}.check{display:flex;align-items:center}.diagnostic{background:#512c2c;color:#ffd0d0;margin:0;padding:10px}.notice{margin:0;padding:8px;background:var(--accent-bg);color:var(--text)}details{margin:8px 0}summary{cursor:pointer;font-size:12px}.add{position:relative}.add[open]{background:var(--surface-raised)}
</style>
