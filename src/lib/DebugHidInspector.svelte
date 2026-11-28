<script lang="ts">
  import {onDestroy} from 'svelte';
  import Icon from './Icon.svelte';
  import {gatewayRequest} from './gateway';
  import {HidInspectionController,type HidInspectionState,type HidRecord} from './controller-hid';
  import HidContract from './hid-inspector/HidContract.svelte';
  import HidMonitor from './hid-inspector/HidMonitor.svelte';
  import HidOperations from './hid-inspector/HidOperations.svelte';
  import VjoyDebug from './hid-inspector/VjoyDebug.svelte';
  import {appendReports,exact,object,rows} from './hid-inspector/model';
  let {active=true}:{active?:boolean}=$props();
  let controller=$state.raw<HidInspectionController|undefined>();
  let view=$state.raw<HidInspectionState|null>(null);
  let monitoring=$state(false),history=$state.raw<HidRecord[]>([]),previous=$state.raw<HidRecord[]>([]);
  let lastSession='',lastCurrent:HidRecord|null=null;
  const label=(row:HidRecord)=>typeof row.product==='string'?row.product:typeof row.name==='string'?row.name:String(row.interfaceId??'Unavailable interface');
  const accessLabel=(row:HidRecord)=>{
    if(typeof row.access==='string')return row.access;
    const access=object(row.access);
    return [access.policy,access.role??row.role].filter(value=>typeof value==='string').join(' · ')||'unprobed';
  };
  function update(value:HidInspectionState){
    const key=exact(value.session??value.replay?.replayId??value.selected);
    if(lastSession!==key){lastSession=key;history=[];previous=[];lastCurrent=null;monitoring=false;}
    if(value.current&&value.current!==lastCurrent){previous=rows(object(lastCurrent?.result).values);lastCurrent=value.current;}
    history=appendReports(history,rows(object(value.reports?.result).reports));view=value;
  }
  $effect(()=>{
    if(!active)return;
    const current=new HidInspectionController(async request=>(await gatewayRequest(request)).controllers,value=>{if(controller===current)update(value);});
    controller=current;view=current.state;
    return()=>{if(controller===current)controller=undefined;void current.dispose();};
  });
  // Display reads are explicit. Native recording owns reports independently of this bounded window.
  $effect(()=>{
    if(!active||!monitoring||!view?.session||view.busy)return;
    const timer=setTimeout(()=>{void controller?.read();},250);
    return()=>clearTimeout(timer);
  });
  onDestroy(()=>{void controller?.dispose();});
  const text=(value:unknown)=>JSON.stringify(value,null,2);
</script>

<section class="panel hid-inspector" aria-label="HID service inspection">
  <div class="heading"><div><h2>HID interfaces</h2><p>Inspect exact native contracts and received observations. Select an interface, then open it explicitly.</p></div><button class="secondary-button" disabled={!view||view.busy} onclick={()=>controller?.refresh()}><Icon name="refresh" size={14}/>Refresh</button></div>
  {#if view}
    <label>Interface<select aria-label="HID interface" value={`${view.cached?'cached':'live'}:${view.selected}`} disabled={view.busy} onchange={event=>{const [kind,...id]=event.currentTarget.value.split(':');void controller?.select(id.join(':'),kind==='cached');}}>
      <option value="live:">Select an interface</option>
      <optgroup label="Current inventory">{#each view.interfaces as row,index (`live/${index}`)}{#if typeof row.interfaceId==='string'}<option value={`live:${row.interfaceId}`}>{label(row)} · {accessLabel(row)}</option>{/if}{/each}</optgroup>
      <optgroup label="Last observed · offline">{#each view.snapshots as row,index (`cached/${index}`)}{#if typeof row.interfaceId==='string'}<option value={`cached:${row.interfaceId}`}>{label(row)} · last observed</option>{/if}{/each}</optgroup>
    </select></label>
    <div class="actions"><button class="secondary-button" disabled={view.busy||!view.detail||view.cached||!!view.session||!!view.replay||!!view.retired} onclick={()=>controller?.open()}><Icon name="activity" size={14}/>Open</button><button class="secondary-button" disabled={view.busy||!view.session} onclick={()=>controller?.read()}><Icon name="refresh" size={14}/>Read capture</button><button class="secondary-button" disabled={view.busy||!view.session} onclick={()=>controller?.close()}><Icon name="close" size={14}/>Close</button><button class="secondary-button" disabled={!view.session} onclick={()=>monitoring=!monitoring}><Icon name="activity" size={14}/>{monitoring?'Pause monitoring':'Start monitoring'}</button></div>
    {#if view.error}<p class="error" role="alert">{view.error}</p>{/if}
    {#if view.busy}<p role="status">Reading native HID service…</p>{/if}
    {#if view.cached&&view.detail}<p role="status">Last observed descriptor · offline. This snapshot does not establish current availability.</p>{/if}
    {#if view.session}<p role="status">Live capture generation: {view.session.generation}{view.ownsSession?'':' · shared capture; Close detaches this view'} · {monitoring?'continuous monitoring':'display paused; native capture continues'}</p>{/if}
    {#if view.retired}<p role="status" aria-label="Historical HID generation">Historical generation: {view.retired.generation} · {view.retired.status}. {exact(view.retired.reason)} Live values were cleared and monitoring stopped. Refresh and explicitly reselect the interface to inspect a replacement; retained recordings remain available below.</p><button class="secondary-button" disabled={view.busy} onclick={async()=>{const selected=view?.selected;if(selected){await controller?.refresh();await controller?.select(selected);}}}><Icon name="refresh" size={14}/>Reselect</button>{/if}
    {#if controller}<HidOperations {controller} {view} clearHistory={()=>{history=[];previous=[];}}/>{/if}
    {#if view.detail}<HidContract detail={view.detail}/><HidMonitor detail={view.detail} current={view.current} reports={history} evidence={object(view.reports?.result)} {previous}/><details><summary>Native contract</summary><pre>{text(view.detail)}</pre></details>{/if}
    {#if view.current}<details><summary>Current decoded state</summary><pre>{text(view.current)}</pre></details>{/if}
    {#if view.reports}<details><summary>Captured reports · current page</summary><pre>{text(view.reports)}</pre></details>{/if}
  {/if}
</section>

<VjoyDebug {active}/>

<style>
  .hid-inspector{padding:var(--ui-panel-padding);margin-bottom:var(--ui-section-gap)}.heading{display:flex;justify-content:space-between;align-items:start;gap:16px}.heading h2{font-size:var(--ui-heading-size);margin:0}.heading p,p,label,summary{color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}label{display:grid;gap:8px}select{width:100%;min-height:var(--ui-control-height);font-size:var(--ui-text-size)}.actions{display:flex;flex-wrap:wrap;gap:8px;margin-top:12px}button{display:inline-flex;align-items:center;gap:6px}details{margin-top:16px}summary{cursor:pointer}.error{color:#d3a078}pre{max-height:360px;overflow:auto;font-size:var(--ui-text-size);line-height:var(--ui-line-height);white-space:pre-wrap;overflow-wrap:anywhere}@media(max-width:600px){.heading{flex-wrap:wrap}}
</style>
