<script lang="ts">
  import Icon from '../Icon.svelte';
  import {closeVjoyCaptureOnExit,gatewayRequest,type GatewayRequest} from '../gateway';
  import {exact,object,rows} from './model';
  import type {VjoyDebugCommand,VjoyDebugResult} from './vjoy-debug';
  let {active=true}:{active?:boolean}=$props();
  let slot=$state(1),busy=$state(false),error=$state(''),monitoring=$state(false);
  let capabilities=$state.raw<Record<string,unknown>|null>(null),page=$state.raw<Record<string,unknown>|null>(null);
  let session=$state.raw<{slot:number;generation:string}|null>(null);
  let ownsSession=$state(false);
  let lifecycle=0;
  let recording=$state.raw<Record<string,unknown>|null>(null),archive=$state.raw<Record<string,unknown>|null>(null);
  let after=$state(''),limit=$state(128),mode=$state<'live'|'replay'>('live');
  const validCursor=$derived(after===''||/^(0|[1-9]\d*)$/.test(after));
  const validSlot=$derived(Number.isInteger(slot)&&slot>=1&&slot<=16);
  const text=(value:unknown)=>JSON.stringify(value,null,2);
  async function request(operation:string,fields:Record<string,unknown>={}){
    return object((await gatewayRequest({command:`controller_vjoy_debug_${operation}` as VjoyDebugCommand,...fields} as GatewayRequest)).controllers) as VjoyDebugResult & Record<string,unknown>;
  }
  async function run(work:()=>Promise<void>){
    if(busy)return;busy=true;error='';
    try{await work();}catch(reason){monitoring=false;error=String(reason);}finally{busy=false;}
  }
  function inspect(){void run(async()=>{
    capabilities=await request('inspect',{slot});
    if(!session&&typeof capabilities.captureGeneration==='string'){
      session={slot,generation:capabilities.captureGeneration};ownsSession=false;mode='live';after='';
    }
  });}
  function open(){void run(async()=>{
    const epoch=lifecycle;
    const result=await request('open',{slot});
    if(typeof result.generation!=='string')throw Error('Native capture did not return a generation.');
    if(epoch!==lifecycle){await request('close',{slot,generation:result.generation});return;}
    session={slot,generation:result.generation};ownsSession=true;capabilities=object(result.capabilities);page=null;recording=null;after='';mode='live';
  });}
  async function read(){await run(async()=>{
    const cursor=after===''?{}:{after};
    page=mode==='replay'?await request('replay',{recording:archive,...cursor,limit}):await request('read',{...session,...cursor,limit});
    if(page.recording)recording=object(page.recording);
    if(mode==='live'&&page.status&&page.status!=='open'&&page.status!=='capturing')monitoring=false;
  });}
  async function close(){await run(async()=>{if(ownsSession)await request('close',{...session});session=null;ownsSession=false;monitoring=false;recording=null;if(mode==='live')page=null;});}
  function record(operation:'start'|'stop'){void run(async()=>{const result=await request(`record_${operation}`,{...session});recording=object(result.recording);});}
  async function exportFile(){await run(async()=>{
    const result=await request('export',{...session});
    const contents=result.recording;
    if(object(contents).format!=='osg-vjoy-ffb')throw Error('Native export did not return a vJoy FFB recording.');
    const url=URL.createObjectURL(new Blob([JSON.stringify(contents)],{type:'application/json'})),link=document.createElement('a');
    link.href=url;link.download=`vjoy-ffb-slot-${session?.slot}.json`;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  });}
  async function importFile(event:Event){
    const input=event.currentTarget as HTMLInputElement,file=input.files?.[0];if(!file)return;
    await run(async()=>{
      if(file.size>384000)throw Error('Recording exceeds the 384,000 byte import limit.');
      const value=object(JSON.parse(await file.text()));
      const result=await request('replay',{recording:value,limit});
      archive=value;page=result;mode='replay';after='';monitoring=false;
    });input.value='';
  }
  $effect(()=>{
    if(!active||!monitoring||busy||!session||mode!=='live')return;
    const timer=setTimeout(()=>{after=typeof page?.next==='string'?page.next:after;void read();},250);
    return()=>clearTimeout(timer);
  });
  $effect(()=>{
    if(!active)return;
    const release=()=>{
      lifecycle++;
      monitoring=false;
      const current=session,owned=ownsSession;session=null;ownsSession=false;
      if(current&&owned)closeVjoyCaptureOnExit(current.slot,current.generation);
    };
    window.addEventListener('pagehide',release);
    return()=>{window.removeEventListener('pagehide',release);release();};
  });
</script>

<section class="panel vjoy-debug" aria-label="vJoy force feedback inspection">
  <h2>vJoy force feedback</h2>
  <p>Inspect the installed SDK and receive force feedback commands for an existing slot. Capture observes incoming commands; replay decodes a file offline.</p>
  <div class="controls">
    <label>Slot<input aria-label="vJoy slot" type="number" min="1" max="16" bind:value={slot} disabled={busy||!!session}/></label>
    <button class="secondary-button" disabled={busy||!validSlot} onclick={inspect}><Icon name="search" size={14}/>Inspect</button>
    <button class="secondary-button" disabled={busy||!validSlot||!!session} onclick={open}><Icon name="activity" size={14}/>Open</button>
    <button class="secondary-button" disabled={busy||!session} onclick={close}><Icon name="close" size={14}/>Close</button>
  </div>
  {#if error}<p role="alert" aria-label="vJoy error">{error}</p>{/if}
  {#if busy}<p role="status">Reading native vJoy service…</p>{/if}
  {#if capabilities}<details open><summary>SDK capabilities</summary><pre aria-label="vJoy capabilities">{text(capabilities)}</pre></details>{/if}
  {#if session}<p role="status">Slot {session.slot} · capture generation {session.generation} · {ownsSession?'owned capture':'shared capture; Close detaches this view'} · {monitoring?'monitoring':'display paused; native capture continues'}</p>{/if}
  {#if session||archive}
    <div class="controls">
      <label>Source<select aria-label="vJoy source" bind:value={mode} disabled={busy} onchange={()=>{monitoring=false;page=null;after='';}}><option value="live" disabled={!session}>Live capture</option><option value="replay" disabled={!archive}>Offline replay</option></select></label>
      <label>After sequence<input aria-label="vJoy cursor" inputmode="numeric" placeholder="Beginning" bind:value={after}/></label>
      <label>Page size<input aria-label="vJoy page size" type="number" min="1" max="128" bind:value={limit}/></label>
    </div>
    <div class="actions">
      <button class="secondary-button" aria-label="Read vJoy capture" disabled={busy||!validCursor||!Number.isInteger(limit)||limit<1||limit>128||(mode==='live'&&!session)} onclick={read}><Icon name="refresh" size={14}/>Read</button>
      <button class="secondary-button" disabled={busy||!page?.hasMore} onclick={()=>{after=String(page?.next??'');void read();}}><Icon name="arrow" size={14}/>Next</button>
      <button class="secondary-button" disabled={busy} onclick={()=>{after='';void read();}}><Icon name="undo" size={14}/>Rewind</button>
      {#if mode==='live'}<button class="secondary-button" disabled={busy||!session} onclick={()=>monitoring=!monitoring}><Icon name="activity" size={14}/>{monitoring?'Pause monitoring':'Start monitoring'}</button>{/if}
    </div>
  {/if}
  {#if session}<section aria-label="vJoy recording"><h3>Recording</h3><p>Bounded to 512 commands and 180,000 raw row bytes. Stop and export before closing or leaving this view.</p>
    <div class="actions"><button class="secondary-button" aria-label="Start vJoy recording" disabled={busy||!ownsSession||recording?.status==='recording'} onclick={()=>record('start')}><Icon name="activity" size={14}/>Start</button><button class="secondary-button" aria-label="Stop vJoy recording" disabled={busy||!ownsSession||recording?.status!=='recording'} onclick={()=>record('stop')}><Icon name="stop" size={14}/>Stop</button><button class="secondary-button" aria-label="Export vJoy recording" disabled={busy||!recording||recording.status!=='stopped'} onclick={exportFile}><Icon name="download" size={14}/>Export</button></div>
    {#if recording}<pre aria-label="vJoy recording status">{text(recording)}</pre>{/if}
  </section>{/if}
  <label>Import recording · JSON, maximum 384,000 bytes<input aria-label="Import vJoy recording" type="file" accept="application/json,.json" disabled={busy} onchange={importFile}/></label>
  {#if archive&&mode==='replay'}<p role="status">Offline replay · slot {exact(archive.slot)} · {rows(archive.records).length} recorded commands</p>{/if}
  {#if page}
    <p role="status">{exact(page.status)} · oldest {exact(page.oldest)} · latest {exact(page.latest)} · next {exact(page.next)} · {page.hasMore?'more available':'end of page stream'}</p>
    {#if page.error}<p role="alert" aria-label="vJoy capture error">Capture ended: {exact(page.error)}</p>{/if}
    {#if page.priorCallbacksExcluded}<p>Recording started after capture began. Replay reconstructs effect state from recorded requests only.</p>{/if}
    {#if page.gap}<p role="alert">Capture gap: {exact(page.gap)}. Earlier commands are unavailable; effect state may be incomplete.</p>{/if}
    <h3>Effects</h3><pre aria-label="vJoy effects">{text(page.effects)}</pre>
    <h3>Commands</h3>
    {#each rows(page.records) as row,index (index)}<details><summary>#{exact(row.sequence)} · command {exact(row.command)} · {exact(object(row.event).reportName??object(row.event).kind??object(row.event).type??'decoded event')} · {exact(object(row.event).status)}</summary><pre>{text(row)}</pre></details>{/each}
    <details><summary>Native capture page</summary><pre aria-label="vJoy native page">{text(page)}</pre></details>
  {/if}
</section>

<style>
  .vjoy-debug{padding:var(--ui-panel-padding);margin-bottom:var(--ui-section-gap)}h2,h3{font-size:var(--ui-heading-size)}h2{margin-top:0}p,label,summary{color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}label{display:grid;gap:8px}input,select{width:100%;min-height:var(--ui-control-height);font-size:var(--ui-text-size)}.controls,.actions{display:flex;flex-wrap:wrap;gap:8px;margin:12px 0}.controls{align-items:end}.controls label{max-width:220px}button{display:inline-flex;align-items:center;gap:6px}details{margin:12px 0}summary{cursor:pointer}pre{max-height:320px;overflow:auto;font-size:var(--ui-text-size);white-space:pre-wrap;overflow-wrap:anywhere}[role=alert]{color:#d3a078}
</style>
