<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { flightTemplate, racingTemplate, validateDesignerDocument, publishDocument, type AssetPayload } from '@opensimgear/companion-designer';
  import { PanelCanvas, createPreviewController, type FeedbackSamples } from '@opensimgear/companion-ui';
  import { invoke } from '@tauri-apps/api/core';
  import { createOrderedInputQueue } from './lib/input-queue.ts';
  interface NativeStatus { phase:string; protectedStoreAvailable?:boolean; epoch:number; busy?:boolean; pairedHost?:{hostId:string;endpoint:string;caSpkiSha256:string;name:string};requestId?:string;assignments?:unknown[];session?:Record<string,unknown>;publicationHash?:string;lastError?:string;ack?:{ackStage?:string;submitted?:boolean;sequence?:string};feedback?:Record<string,unknown>[];feedbackRevision?:number }
  let panel = $state(flightTemplate), pageId = $state(flightTemplate.ui.pages[0].id);
  let remote = $state<NativeStatus>({phase:'unpaired',epoch:0});
  let nativeAvailable = $state(false), nativeStatus = $state('Browser local preview'), remoteSelected = $state(false);
  let error = $state(''), pairingPayload = $state(''), displayName = $state('Desktop panel'), instanceId = $state('');
  let assets = $state<Record<string,string>>({}), feedback = $state<FeedbackSamples>({}), hints = $state<unknown[]>([]);
  let loadedHash = '', epoch:number|null=null, switching=false, polling=false, disposed=false;
  let feedbackSequence = '';
  const request = <T,>(value:unknown) => invoke<T>('companion_client_request',{request:value});
  const inputQueue=createOrderedInputQueue((epoch,changes)=>request({type:'input',epoch,changes}),reason=>fail(new Error(reason)));
  const recorder = createPreviewController(flightTemplate,{onchange:(changes)=>{
    if(remoteSelected && remote.phase==='armed') inputQueue.enqueue(remote.epoch,changes);
  }});
  let snapshot = $state(recorder.snapshot());
  const validation = $derived(validateDesignerDocument(panel));
  const assigned = $derived((remote.assignments??[]).map(value=>typeof value==='string'?{id:value,label:value}:value && typeof value==='object'?{id:String((value as Record<string,unknown>).instanceId??(value as Record<string,unknown>).id??''),label:String((value as Record<string,unknown>).label??(value as Record<string,unknown>).name??(value as Record<string,unknown>).instanceId??(value as Record<string,unknown>).id??'Assigned panel')}:{id:'',label:''}).filter(value=>value.id));
  function fail(value:unknown){inputQueue.stop();error=value instanceof Error?value.message:String(value);remote={...remote,phase:'error'};recorder.disarm('Native connection failed');if(nativeAvailable)void request({type:'neutralize'}).catch(()=>{});}
  function disarm(reason:string){inputQueue.stop();snapshot=recorder.disarm(reason);if(remoteSelected&&['ready','armed','connecting'].includes(remote.phase))void request({type:'neutralize'}).catch(()=>{});}
  async function togglePreview(){
    if(snapshot.armed){disarm('Stopped by operator');return;}
    switching=true;error='';
    try{await tick();if(document.visibilityState!=='visible'||!document.hasFocus())return;
      if(remoteSelected){if(remote.phase!=='ready'||loadedHash!==remote.publicationHash)throw new Error('Connect and load a fresh acknowledged publication first');await request({type:'arm',epoch:remote.epoch});remote={...remote,phase:'armed'};inputQueue.start();}
      snapshot=recorder.arm();
    }catch(value){fail(value);}finally{switching=false;}
  }
  async function poll(){
    if(!nativeAvailable||polling||disposed)return;polling=true;
    try{const next=await request<NativeStatus>({type:'status'});if(disposed)return;remote=next;
      if(remoteSelected&&next.phase!=='armed'&&snapshot.armed){inputQueue.stop();recorder.disarm('Remote session is disarmed');}
      if(next.publicationHash&&next.publicationHash!==loadedHash&&['ready','armed'].includes(next.phase)){
        const published=await request<{canonicalDocument:string;document:unknown;assets:AssetPayload[];publicationHash:string}>({type:'publication'});
        const checked=validateDesignerDocument(published.document);if(!checked.ok)throw new Error(checked.error);
        const verified=await publishDocument(checked.value,published.assets);if(verified.publicationHash!==published.publicationHash)throw new Error('Published layout hash differs from the verified native transfer');
        const urls:Record<string,string>={};for(const payload of published.assets){const manifest=checked.value.ui.assets.find(value=>value.id===payload.assetId)!;urls[payload.assetId]=`data:${manifest.mime};base64,${payload.base64}`;}
        recorder.disarm('Fresh publication loaded');panel=checked.value;pageId=panel.ui.pages[0].id;assets=urls;loadedHash=published.publicationHash;
      }
      const sequence=`${next.session?.sessionId??''}:${next.feedbackRevision??0}`;
      if(sequence!==feedbackSequence&&next.feedback){const samples:FeedbackSamples={};for(const value of next.feedback){if(typeof value.channelId==='string')samples[value.channelId]={...value,receivedAtMs:performance.now()} as FeedbackSamples[string];}feedback=samples;feedbackSequence=sequence;}
      if(!instanceId&&assigned.length)instanceId=assigned[0].id;
    }catch(value){fail(value);}finally{polling=false;}
  }
  async function pair(){error='';recorder.disarm('Pairing requested');try{await request({type:'pair-start',payload:pairingPayload,name:displayName});pairingPayload='';await poll();}catch(value){fail(value);}}
  async function connect(){error='';recorder.disarm('Connecting to host');remoteSelected=true;loadedHash='';assets={};feedback={};try{await request({type:'connect',instanceId});await poll();}catch(value){fail(value);}}
  async function disconnect(){disarm('Disconnected by operator');try{await request({type:'disconnect'});await poll();}catch(value){fail(value);}}
  async function local(){await disconnect();remoteSelected=false;panel=flightTemplate;pageId=panel.ui.pages[0].id;assets={};feedback={};}
  async function discover(){error='';try{const result=await request<{hints:unknown[]}>({type:'discover',durationMs:1200});hints=result.hints;}catch(value){fail(value);}}
  async function forget(){await disconnect();try{await request({type:'forget'});remoteSelected=false;await poll();}catch(value){fail(value);}}
  function visibility(){if(document.visibilityState==='hidden')disarm('Page hidden');}
  onMount(()=>{
    disposed=false;
    const unsubscribe=recorder.subscribe(state=>{snapshot=state;if(!switching&&!state.armed&&remoteSelected&&remote.phase==='armed'){inputQueue.stop();void request({type:'neutralize'}).catch(()=>{});}});
    let unlisten:(()=>void)|undefined;
    async function synchronizeNative(){try{const state=await invoke<{invalidationEpoch:number}>('companion_lifecycle');if(disposed)return;nativeAvailable=true;nativeStatus='Tauri 2 · native pinned WSS';if(epoch===null||epoch!==state.invalidationEpoch)disarm('Native lifecycle changed');epoch=state.invalidationEpoch;await poll();}catch{nativeAvailable=false;}}
    async function initialize(){try{const{listen}=await import('@tauri-apps/api/event');const stop=await listen('companion-lifecycle-invalidated',()=>{disarm('Native lifecycle changed');void synchronizeNative();});if(disposed)stop();else unlisten=stop;}catch{}await synchronizeNative();}
    const focus=()=>{void synchronizeNative();};window.addEventListener('focus',focus);void initialize();
    const statusTimer=setInterval(()=>void poll(),250);
    const leaseTimer=setInterval(()=>{if(remoteSelected&&remote.phase==='armed'&&snapshot.armed)void request({type:'keepalive',epoch:remote.epoch}).catch(fail);},100);
    return()=>{disposed=true;inputQueue.stop();clearInterval(statusTimer);clearInterval(leaseTimer);unsubscribe();unlisten?.();window.removeEventListener('focus',focus);recorder.disarm('Renderer unmounted');if(nativeAvailable)void request({type:'neutralize'}).catch(()=>{});};
  });
</script>
<svelte:window onblur={()=>disarm('Window lost focus')} onpagehide={()=>disarm('Page unloading')} />
<svelte:document onvisibilitychange={visibility} />
<div class="app-shell">
  <header class="app-header"><a class="brand" href="#panel" aria-label="OpenSimGear Companion"><span class="brand-mark">OSG</span><span>OpenSimGear <strong>Companion</strong></span></a><span class="prototype-badge">C2 NATIVE COMPANION</span></header>
  <main id="panel">
    <div class="intro"><div><span class="eyebrow">YOUR PANEL. ANY SCREEN.</span><h1>A cockpit within reach.</h1><p>Pair directly with your Cockpit host, or explore a local panel.</p></div><div class="target-badges"><span class="target-active">Desktop</span><span>Android · pending</span><span>iOS · pending</span></div></div>
    <div class="session-bar"><div class="session-label"><span class="status-dot" class:live={snapshot.armed}></span><div><strong>{snapshot.armed?(remoteSelected?'Remote panel active':'Local preview running'):'Disarmed'}</strong><span>{remoteSelected?`${remote.phase} · ${remote.ack?.ackStage??'waiting for baseline'}`:'Inputs are recorded on this device only.'}</span></div></div><button class="preview-toggle" class:running={snapshot.armed} onclick={togglePreview} disabled={!validation.ok||(remoteSelected&&remote.phase!=='ready'&&!snapshot.armed)}>{snapshot.armed?'Stop':remoteSelected?'Start panel':'Start local preview'}<span>{snapshot.armed?'■':'↗'}</span></button></div>
    <div class="template-chooser">{#if !remoteSelected}<label>Reference panel <select onchange={(event)=>{panel=event.currentTarget.value==='racing'?racingTemplate:flightTemplate;pageId=panel.ui.pages[0].id;}}><option value="flight">Flight instruments</option><option value="racing">Racing dashboard</option></select></label>{:else}<span>Authenticated host publication · {remote.publicationHash?.slice(0,24)??'loading'}</span>{/if}<label>Page <select bind:value={pageId}>{#each panel.ui.pages as page}<option value={page.id}>{page.title}</option>{/each}</select></label><span>{remoteSelected?'Native session lease · explicit Start':'Local input only'}</span></div>
    {#if error}<p class="validation-error" role="alert">{error}</p>{/if}
    {#if validation.ok}<PanelCanvas document={panel} {pageId} {assets} {feedback} controller={recorder}/>{:else}<p class="validation-error">Panel validation failed: {validation.error}</p>{/if}
    <div class="bottom-grid">
      <section class="connection-card"><div class="card-heading"><h3>Connection</h3><span class="pill">{nativeAvailable?remote.phase.toUpperCase():'BROWSER PREVIEW'}</span></div>
        {#if nativeAvailable}
          {#if remote.pairedHost}<p><strong>Cockpit host</strong><br/>{remote.pairedHost.endpoint}</p><p class="fingerprint">Host CA · {remote.pairedHost.caSpkiSha256}</p><div class="connection-actions"><label>Assigned controller <select bind:value={instanceId}>{#each assigned as item}<option value={item.id}>{item.label}</option>{/each}</select></label><button onclick={connect} disabled={!instanceId||remote.busy||['pairing','awaiting-approval','connecting','ready','armed'].includes(remote.phase)}>Connect</button><button onclick={()=>request({type:'refresh-assignments'}).then(()=>poll()).catch(fail)} disabled={['armed','ready','connecting'].includes(remote.phase)}>Refresh assignments</button><button onclick={disconnect}>Disconnect</button><button onclick={local}>Local preview</button><button onclick={forget}>Forget host</button></div>
          {:else}<p>Paste the pairing payload shown by your Cockpit host. The host CA fingerprint is checked before native code sends the one-time secret.</p><label class="pair-field">This device name<input bind:value={displayName} maxlength="80"/></label><label class="pair-field">Manual / decoded QR payload<textarea bind:value={pairingPayload} rows="3" placeholder="Paste the host's pairing payload" spellcheck="false"></textarea></label><button class="connection-primary" onclick={pair} disabled={remote.protectedStoreAvailable===false||!pairingPayload||['pairing','awaiting-approval'].includes(remote.phase)}>Request pairing</button>{/if}
          {#if remote.protectedStoreAvailable===false}<p>Native pairing waits for protected storage qualification on this platform. Local preview remains available.</p>{/if}{#if remote.requestId}<p>Approve request <code>{remote.requestId}</code> in Cockpit. Pairing can complete while this window is in the background.</p>{/if}
          {#if remote.lastError}<p class="validation-error">{remote.lastError}</p>{/if}
          <button class="discover-button" onclick={discover}>Find nearby hosts</button>{#if hints.length}<pre class="discovery-hints">{JSON.stringify(hints,null,2)}</pre>{/if}<p class="fine-print">Discovery is an untrusted hint. Use a payload received directly from your host. Credentials stay in native protected storage. Backgrounding stops an active session; it never resumes automatically. If an instance is busy, use Cockpit’s host-side Disconnect/Takeover controls.</p>
        {:else}<p>Native pairing is available in the desktop Companion app. Browser preview records locally and has no network access.</p>{/if}
      </section>
      <section class="diagnostics-card"><div class="card-heading"><h3>Input recorder</h3><span class="pill">{snapshot.events.length} RECENT EVENTS</span></div><div class="diagnostic-stats"><div><strong>{snapshot.activePointers}</strong><span>active pointers</span></div><div><strong>{snapshot.armed?(remoteSelected?'Remote':'Preview'):'Disarmed'}</strong><span>session state</span></div></div><p class="last-reason">{snapshot.reason}</p>{#if remoteSelected}<p class="fine-print">Acknowledgement: {remote.ack?.ackStage??'none'} · native submission: {remote.ack?.submitted?'reported':'not claimed'}. Game confirmation requires a game feedback source.</p>{/if}<div class="event-log" role="log" aria-label="Recent input changes">{#each snapshot.events.slice(-3).reverse() as entry}<div><span>#{entry.sequence}</span><code>{entry.changes.map(change=>`${change.controlId}: ${change.value}`).join(' · ')}</code></div>{:else}<div class="empty-log">Control transitions will appear here.</div>{/each}</div></section>
    </div>
  </main><footer><span>{nativeStatus}</span><span>Stop and lifecycle loss clear held input. Reconnect and Start explicitly.</span></footer>
</div>
