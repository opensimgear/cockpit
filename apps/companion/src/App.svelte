<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { flightTemplate, racingTemplate, validateDesignerDocument, type Publication } from '@opensimgear/companion-designer';
  import { PanelCanvas, createPreviewController, type FeedbackSamples } from '@opensimgear/companion-ui';
  import { invoke } from '@tauri-apps/api/core';
  import { createOrderedInputQueue } from './lib/input-queue.ts';
  import { browserCacheStore, cacheKey, createPublicationCache, verifyPublishedPayload, type CacheSummary, type PublishedPayload } from './lib/companion-cache.ts';
  import { createSessionGuard, remoteIdentity, type PanelMode } from './lib/session-guard.ts';
  interface NativeStatus { phase:string; protectedStoreAvailable?:boolean; authenticationAvailable?:boolean; authenticationUnavailableReason?:string|null; storageError?:string|null; epoch:number; busy?:boolean; pairedHost?:{hostId:string;endpoint:string;caSpkiSha256:string;name:string};requestId?:string;assignments?:unknown[];session?:Record<string,unknown>;publicationHash?:string;lastError?:string;ack?:{ackStage?:string;submitted?:boolean;sequence?:string};feedback?:Record<string,unknown>[];feedbackRevision?:number }
  let panel = $state(flightTemplate), pageId = $state(flightTemplate.ui.pages[0].id), reference = $state('flight');
  let remote = $state<NativeStatus>({phase:'unpaired',epoch:0}), panelMode = $state<PanelMode>('local');
  let nativeAvailable = $state(false), nativeStatus = $state('Browser local preview');
  let error = $state(''), pairingPayload = $state(''), displayName = $state('Portable panel'), instanceId = $state('');
  let assets = $state<Record<string,string>>({}), feedback = $state<FeedbackSamples>({}), hints = $state<unknown[]>([]);
  let cached = $state<CacheSummary[]>([]), offlineKey = $state(''), currentCacheKey = $state(''), cacheNotice = $state('Loading saved panels...');
  let loadedHash = $state(''), sessionUpdate = $state(0), switching = $state(false);
  let epoch:number|null=null, polling=false, disposed=false, feedbackSequence='';
  let discovering=$state(false);
  const gate=createSessionGuard(), cache=createPublicationCache(browserCacheStore());
  const remoteSelected=$derived(panelMode==='remote'), offline=$derived(panelMode==='offline');
  const request = <T,>(value:unknown) => invoke<T>('companion_client_request',{request:value});
  const inputQueue=createOrderedInputQueue((epoch,changes)=>request({type:'input',epoch,changes}),reason=>fail(new Error(reason)));
  const recorder = createPreviewController(flightTemplate,{onchange:changes=>{
    if(remoteSelected&&gate.snapshot().armed&&remote.phase==='armed')inputQueue.enqueue(remote.epoch,changes);
  }});
  let snapshot=$state(recorder.snapshot());
  const validation=$derived(validateDesignerDocument(panel));
  const assigned=$derived((remote.assignments??[]).map(value=>typeof value==='string'?{id:value,label:value}:value&&typeof value==='object'?{id:String((value as Record<string,unknown>).instanceId??(value as Record<string,unknown>).id??''),label:String((value as Record<string,unknown>).label??(value as Record<string,unknown>).name??(value as Record<string,unknown>).instanceId??'Assigned panel')}:{id:'',label:''}).filter(value=>value.id));
  const canStart=$derived.by(()=>{sessionUpdate;return !switching&&validation.ok&&(panelMode==='local'||remoteSelected&&gate.canStart(remote,true));});
  const savedPanel=$derived(cached.find(entry=>entry.key===currentCacheKey&&entry.publicationHash===loadedHash));
  function visible(){return document.visibilityState==='visible'&&document.hasFocus();}
  function message(value:unknown){return value instanceof Error?value.message:String(value);}
  function invalidate(reason:string,mode:PanelMode=panelMode,selected=instanceId,close=true){
    const shouldClose=remoteSelected&&['ready','armed','connecting'].includes(remote.phase);
    gate.invalidate(mode,selected);sessionUpdate++;panelMode=mode;inputQueue.stop();feedback={};feedbackSequence='';snapshot=recorder.disarm(reason);
    if(close&&shouldClose&&nativeAvailable)void request({type:'neutralize'}).catch(()=>{});
    return gate.capture();
  }
  function fail(value:unknown){error=message(value);const mode=remoteSelected&&loadedHash?'offline':panelMode;remote={...remote,phase:'error'};invalidate('Connection unavailable; input stopped',mode);if(nativeAvailable)void request({type:'neutralize'}).catch(()=>{});}
  function disarm(reason:string){invalidate(reason);}
  async function refreshCache(){try{cached=await cache.list();if(!cached.some(entry=>entry.key===offlineKey))offlineKey=cached[0]?.key??'';cacheNotice=cached.length?'Saved panels contain public layouts and assets only.':'No verified panel has been saved yet.';}catch(value){cacheNotice=message(value);}}
  function showPublication(publication:Publication,page?:string){
    const urls:Record<string,string>={};for(const payload of publication.assets){const manifest=publication.document.ui.assets.find(value=>value.id===payload.assetId)!;urls[payload.assetId]=`data:${manifest.mime};base64,${payload.base64}`;}
    recorder.disarm('Verified panel loaded');panel=publication.document;pageId=panel.ui.pages.some(value=>value.id===page)?page!:panel.ui.pages[0].id;assets=urls;loadedHash=publication.publicationHash;feedback={};feedbackSequence='';
  }
  async function openOffline(){
    const token=invalidate('Saved panel opened offline','offline');error='';
    try{const entry=await cache.open(offlineKey);if(disposed||!gate.current(token))return;currentCacheKey=entry.key;showPublication(entry.publication,entry.pageId);cacheNotice='Offline view is disarmed. Feedback is unknown; connect to the host for a fresh baseline.';}catch(value){if(gate.current(token))cacheNotice=message(value);}
  }
  async function clearCache(){try{await cache.clear();await refreshCache();if(offline)local();}catch(value){cacheNotice=message(value);}}
  async function togglePreview(){
    if(snapshot.armed){disarm('Stopped by operator');return;}if(offline||switching)return;
    let token=gate.capture();const starting=remote;
    if(remoteSelected){const next=gate.beginStart(starting,visible());if(next===null)return;token=next;sessionUpdate++;}
    else token=invalidate('Starting local preview','local');
    switching=true;error='';
    try{await tick();if(!gate.current(token)||!visible())return;
      if(remoteSelected){await request({type:'arm',epoch:starting.epoch});if(!gate.arm(token,starting,visible()))return;remote={...starting,phase:'armed'};inputQueue.start();sessionUpdate++;}
      if(gate.current(token)&&visible())snapshot=recorder.arm();
    }catch(value){if(gate.current(token))fail(value);}finally{switching=false;}
  }
  async function poll(){
    if(!nativeAvailable||polling||disposed||switching)return;polling=true;const token=gate.capture();
    try{const next=await request<NativeStatus>({type:'status'});if(disposed||!gate.current(token))return;const previousPhase=remote.phase;remote=next;if(['initializing','error'].includes(previousPhase)&&next.phase!=='error'&&!next.lastError)error='';
      if(remoteSelected){
        const verified=gate.snapshot().verified;
        if(verified&&(!gate.matches(next)||!['ready','armed'].includes(next.phase))){invalidate('Remote session ended; reconnect explicitly',loadedHash?'offline':'remote',instanceId,false);return;}
        if(next.phase!=='armed'&&snapshot.armed){invalidate('Remote panel is disarmed',loadedHash?'offline':'remote',instanceId,false);return;}
        if(next.publicationHash&&['ready','armed'].includes(next.phase)&&!gate.matches(next)&&gate.snapshot().acceptingConnection){
          const published=await request<PublishedPayload>({type:'publication'});if(!gate.current(token))return;
          const verifiedPublication=await verifyPublishedPayload(published);if(disposed||!gate.current(token))return;
          if(!gate.verify(token,next,verifiedPublication.publicationHash))throw new Error('Publication belongs to another controller session');
          const identity=remoteIdentity(next)!;const hostId=next.pairedHost?.hostId;if(!hostId)throw new Error('Paired host identity is unavailable');
          const owner={hostId,instanceId:identity.instanceId,name:assigned.find(entry=>entry.id===identity.instanceId)?.label??verifiedPublication.document.ui.title};
          currentCacheKey=cacheKey(owner);const previous=cached.find(entry=>entry.key===currentCacheKey&&entry.publicationHash===verifiedPublication.publicationHash);
          showPublication(verifiedPublication,previous?.pageId);sessionUpdate++;
          void cache.save(owner,verifiedPublication,pageId).then(refreshCache).catch(value=>{if(gate.current(token))cacheNotice=`Panel is usable online but could not be saved: ${message(value)}`;});
        }
        if(gate.matches(next)&&['ready','armed'].includes(next.phase)){
          const sequence=`${next.epoch}:${next.session?.sessionId??''}:${next.feedbackRevision??0}`;
          if(sequence!==feedbackSequence){const samples:FeedbackSamples={};for(const value of next.feedback??[]){if(typeof value.channelId==='string')samples[value.channelId]={...value,receivedAtMs:performance.now()} as FeedbackSamples[string];}feedback=samples;feedbackSequence=sequence;}
        }else feedback={};
      }
      if(instanceId&&!assigned.some(entry=>entry.id===instanceId)){invalidate('Controller assignment changed',loadedHash?'offline':panelMode);instanceId=assigned[0]?.id??'';}
      else if(!instanceId&&assigned.length)instanceId=assigned[0].id;
    }catch(value){if(gate.current(token))fail(value);}finally{polling=false;}
  }
  async function pair(){const token=invalidate('Pairing requested'),payload=pairingPayload;pairingPayload='';error='';try{await request({type:'pair-start',payload,name:displayName});if(gate.current(token))await poll();}catch(value){if(gate.current(token))fail(value);}}
  async function connect(){
    invalidate('Connecting with a fresh baseline','remote',instanceId,false);const token=gate.connect(instanceId);sessionUpdate++;error='';loadedHash='';
    try{await request({type:'connect',instanceId});if(gate.current(token))await poll();}catch(value){if(gate.current(token))fail(value);}
  }
  async function disconnect(){const token=invalidate('Disconnected by operator',loadedHash?'offline':panelMode);try{await request({type:'disconnect'});if(gate.current(token))await poll();}catch(value){if(gate.current(token))fail(value);}}
  function local(){invalidate('Local reference panel selected','local');panel=reference==='racing'?racingTemplate:flightTemplate;pageId=panel.ui.pages[0].id;assets={};loadedHash='';currentCacheKey='';}
  function chooseReference(value:string){reference=value;local();}
  function chooseAssignment(value:string){invalidate('Controller selection changed',loadedHash?'offline':'remote',value);instanceId=value;}
  function choosePage(value:string){if(!panel.ui.pages.some(page=>page.id===value))return;disarm('Panel page changed; reconnect and Start explicitly');pageId=value;if(currentCacheKey)void cache.rememberPage(currentCacheKey,value).then(refreshCache).catch(value=>cacheNotice=message(value));}
  async function refreshAssignments(){const token=invalidate('Refreshing controller assignments',loadedHash?'offline':panelMode);try{await request({type:'refresh-assignments'});if(gate.current(token))await poll();}catch(value){if(gate.current(token))fail(value);}}
  async function retryStorage(){error='';try{await request({type:'storage-status'});await poll();}catch(value){fail(value);}}
  async function discover(){if(discovering)return;discovering=true;error='';const token=gate.capture();try{const result=await request<{hints:unknown[]}>({type:'discover',durationMs:1200});if(gate.current(token))hints=result.hints;}catch(value){if(gate.current(token))error=message(value);}finally{discovering=false;}}
  async function forget(){const token=invalidate('Removing native pairing',loadedHash?'offline':panelMode);try{await request({type:'disconnect'});if(!gate.current(token))return;await request({type:'forget'});if(gate.current(token)){local();await poll();}}catch(value){if(gate.current(token))fail(value);}}
  function visibility(){if(document.visibilityState==='hidden')disarm('Page hidden');}
  onMount(()=>{
    disposed=false;
    const unsubscribe=recorder.subscribe(state=>{snapshot=state;if(!switching&&!state.armed&&gate.snapshot().armed)disarm(state.reason);});
    let unlisten:(()=>void)|undefined;
    async function synchronizeNative(){try{const state=await invoke<{invalidationEpoch:number}>('companion_lifecycle');if(disposed)return;nativeAvailable=true;nativeStatus='Tauri 2 · native pinned WSS';if(epoch===null||epoch!==state.invalidationEpoch)disarm('Native lifecycle changed');epoch=state.invalidationEpoch;await poll();}catch{nativeAvailable=false;}}
    async function initialize(){try{const{listen}=await import('@tauri-apps/api/event');const stop=await listen('companion-lifecycle-invalidated',()=>{disarm('Native lifecycle changed');void synchronizeNative();});if(disposed)stop();else unlisten=stop;}catch{}await synchronizeNative();}
    const focus=()=>{void synchronizeNative();},orientation=()=>disarm('Orientation changed; reconnect and Start explicitly');window.addEventListener('focus',focus);
    const screenOrientation=screen.orientation;if(screenOrientation)screenOrientation.addEventListener('change',orientation);else window.addEventListener('orientationchange',orientation);
    void initialize();void refreshCache();
    const statusTimer=setInterval(()=>void poll(),250),leaseTimer=setInterval(()=>{if(remoteSelected&&gate.snapshot().armed&&remote.phase==='armed'&&snapshot.armed){const token=gate.capture();void request({type:'keepalive',epoch:remote.epoch}).catch(value=>{if(gate.current(token))fail(value);});}},100);
    return()=>{disposed=true;gate.invalidate();inputQueue.stop();clearInterval(statusTimer);clearInterval(leaseTimer);unsubscribe();unlisten?.();window.removeEventListener('focus',focus);if(screenOrientation)screenOrientation.removeEventListener('change',orientation);else window.removeEventListener('orientationchange',orientation);recorder.disarm('Renderer unmounted');if(nativeAvailable)void request({type:'neutralize'}).catch(()=>{});};
  });
</script>
<svelte:window onblur={()=>disarm('Window lost focus')} onpagehide={()=>disarm('Page unloading')} />
<svelte:document onvisibilitychange={visibility} />
<div class="app-shell">
  <header class="app-header"><a class="brand" href="#panel" aria-label="OpenSimGear Companion"><span class="brand-mark">OSG</span><span>OpenSimGear <strong>Companion</strong></span></a><span class="prototype-badge">PORTABLE COMPANION</span></header>
  <main id="panel">
    <div class="intro"><div><span class="eyebrow">YOUR PANEL. ANY SCREEN.</span><h1>A cockpit within reach.</h1><p>Pair with your Cockpit host, or open a saved panel offline.</p></div><div class="target-badges"><span class="target-active">Desktop</span><span>Android · pending</span><span>iOS · pending</span></div></div>
    <div class="session-bar"><div class="session-label"><span class="status-dot" class:live={snapshot.armed}></span><div><strong>{snapshot.armed?(remoteSelected?'Remote panel active':'Local preview running'):offline?'Offline view · Disarmed':'Disarmed'}</strong><span>{offline?'Saved layout only. Feedback is unknown.':remoteSelected?`${remote.phase} · ${remote.ack?.ackStage??'Connect for a fresh baseline'}`:'Inputs are recorded on this device only.'}</span></div></div><button class="preview-toggle" class:running={snapshot.armed} onclick={togglePreview} disabled={!snapshot.armed&&!canStart}>{switching?'Starting...':snapshot.armed?'Stop':offline?'Offline · Disarmed':remoteSelected?'Start panel':'Start local preview'}<span>{snapshot.armed?'■':'↗'}</span></button></div>
    <div class="template-chooser">{#if panelMode==='local'}<label>Reference panel <select value={reference} onchange={event=>chooseReference(event.currentTarget.value)}><option value="flight">Flight instruments</option><option value="racing">Racing dashboard</option></select></label>{:else}<span>{offline?'Saved panel':'Verified host publication'} · {loadedHash?loadedHash.slice(0,24):'waiting for publication'}</span>{/if}<label>Page <select value={pageId} onchange={event=>choosePage(event.currentTarget.value)}>{#each panel.ui.pages as page}<option value={page.id}>{page.title}</option>{/each}</select></label><span>{offline?'Reconnect and Start explicitly':remoteSelected?'Native session lease · explicit Start':'Local input only'}</span></div>
    {#if offline}<p class="offline-banner">{savedPanel?`Saved ${new Date(savedPanel.savedAtMs).toLocaleString()}.`:'This view is held in memory; durable saving may be unavailable.'} Controls stay disarmed and feedback is unavailable until a fresh host connection. <button onclick={local}>Explore local references</button></p>{/if}
    {#if error}<p class="validation-error" role="alert">{error}</p>{/if}
    {#if validation.ok}<PanelCanvas document={panel} {pageId} {assets} {feedback} controller={recorder}/>{:else}<p class="validation-error">Panel validation failed: {validation.error}</p>{/if}
    <section class="offline-library" aria-label="Saved offline panels"><div><strong>Saved panels</strong><span>{cacheNotice}</span></div><label>Panel <select bind:value={offlineKey} disabled={!cached.length}>{#each cached as entry}<option value={entry.key}>{entry.name} · {entry.title} · {entry.hostId.slice(0,8)}</option>{/each}</select></label><button onclick={openOffline} disabled={!offlineKey}>Open offline</button><button onclick={clearCache}>Clear saved panels</button></section>
    <div class="bottom-grid">
      <section class="connection-card"><div class="card-heading"><h3>Connection</h3><span class="pill">{nativeAvailable?remote.phase.toUpperCase():'BROWSER PREVIEW'}</span></div>
        {#if nativeAvailable}
          {#if remote.pairedHost}<p><strong>Cockpit host</strong><br/>{remote.pairedHost.endpoint}</p><p class="fingerprint">Host CA · {remote.pairedHost.caSpkiSha256}</p><div class="connection-actions"><label>Assigned controller <select value={instanceId} onchange={event=>chooseAssignment(event.currentTarget.value)}>{#each assigned as item}<option value={item.id}>{item.label}</option>{/each}</select></label><button onclick={connect} disabled={!instanceId||remote.busy||remote.authenticationAvailable===false||['pairing','awaiting-approval','connecting','ready','armed'].includes(remote.phase)}>Connect</button><button onclick={refreshAssignments} disabled={remote.busy||remote.authenticationAvailable===false||['armed','ready','connecting'].includes(remote.phase)}>Refresh assignments</button><button onclick={disconnect}>Disconnect</button><button onclick={local}>Local preview</button><button onclick={forget}>Forget host</button></div>
          {:else}<p>Paste a pairing payload received directly from your Cockpit host. Native code checks its host CA fingerprint before sending the one-time secret.</p><label class="pair-field">This device name<input bind:value={displayName} maxlength="80"/></label><label class="pair-field">Manual / decoded QR payload<textarea bind:value={pairingPayload} rows="3" placeholder="Paste the host's pairing payload" spellcheck="false"></textarea></label><button class="connection-primary" onclick={pair} disabled={remote.authenticationAvailable===false||remote.protectedStoreAvailable===false||!pairingPayload||remote.busy||['pairing','awaiting-approval'].includes(remote.phase)}>Request pairing</button>{/if}
          {#if remote.authenticationAvailable===false||remote.protectedStoreAvailable===false}<p>{remote.authenticationUnavailableReason??'Native authentication waits for qualified protected storage on this platform. Saved panels and local preview remain available.'}</p><button onclick={retryStorage}>Retry protected storage</button>{#if remote.storageError}<p class="validation-error">{remote.storageError}</p><button onclick={forget}>Forget saved pairing</button>{/if}{/if}{#if remote.requestId}<p>Approve request <code>{remote.requestId}</code> in Cockpit. Pairing can finish while this window is in the background.</p>{/if}
          {#if remote.lastError}<p class="validation-error">{remote.lastError}</p>{/if}
          <button class="discover-button" onclick={discover} disabled={discovering}>{discovering?'Looking nearby...':'Find nearby hosts'}</button>{#if hints.length}<pre class="discovery-hints">{JSON.stringify(hints,null,2)}</pre>{/if}<p class="fine-print">Discovery is an untrusted hint. Credentials stay in native protected storage. Backgrounding, page changes and rotation stop an active session; Connect and Start explicitly afterward. If an instance is busy, use Cockpit’s host-side Disconnect/Takeover controls.</p>
        {:else}<p>Browser preview records locally. Remote input requires the native Companion app and available protected storage. Saved panels remain disarmed offline.</p>{/if}
      </section>
      <section class="diagnostics-card"><div class="card-heading"><h3>Input recorder</h3><span class="pill">{snapshot.events.length} RECENT EVENTS</span></div><div class="diagnostic-stats"><div><strong>{snapshot.activePointers}</strong><span>active pointers</span></div><div><strong>{snapshot.armed?(remoteSelected?'Remote':'Preview'):offline?'Offline':'Disarmed'}</strong><span>session state</span></div></div><p class="last-reason">{snapshot.reason}</p>{#if remoteSelected}<p class="fine-print">Acknowledgement: {remote.ack?.ackStage??'none'} · native submission: {remote.ack?.submitted?'reported':'not claimed'}. Game confirmation requires a game feedback source.</p>{/if}<div class="event-log" role="log" aria-label="Recent input changes">{#each snapshot.events.slice(-3).reverse() as entry}<div><span>#{entry.sequence}</span><code>{entry.changes.map(change=>`${change.controlId}: ${change.value}`).join(' · ')}</code></div>{:else}<div class="empty-log">Control transitions will appear here.</div>{/each}</div></section>
    </div>
  </main><footer><span>{nativeStatus}</span><span>Saved content never grants input authority. Reconnect and Start explicitly.</span></footer>
</div>
