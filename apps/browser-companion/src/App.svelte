<script lang="ts">
  import { onMount, tick } from 'svelte';
  import CompiledPanel from '../../../packages/companion-ui/src/CompiledPanel.svelte';
  import {prepareCompiledSurfaces,disposeCompiledSurfaces,type PreparedCompiledSurfaces} from '../../../packages/companion-ui/src/compiled-surface.ts';
  import { BROWSER_WIDGET_CAPABILITIES } from '../../../packages/widget-runtime/src/artifact.ts';
  import type { CompiledPanelPublication } from '../../../packages/widget-runtime/src/panel.ts';
  import type { AudioAvailability } from '../../../packages/widget-runtime/src/player.ts';
  import { createCompiledAssetCache, browserCompiledCacheStore } from './compiled-cache';
  import { createOrderedInputQueue } from '../../companion/src/lib/input-queue';
  import { BrowserTransport, readCredential, saveCredential, clearCredential, type Credential } from './transport';
  import { createBrowserFeedback, type FeedbackIdentity, type FeedbackSamples } from './feedback';
  import { panelResolutionWarning, panelDisplayReport } from './panel-resolution';
  let credential=$state<Credential|null>(null), error=$state(''), warning=$state(''), status=$state('Connecting…');
  let panel=$state<CompiledPanelPublication|null>(null), pageId=$state('');
  let displayWidth=$state(0), displayHeight=$state(0), pixelRatio=$state(1);
  let windowWidth=$state(0), windowHeight=$state(0);
  const displayedPage=$derived(pageId||panel?.document.ui.pages[0].id);
  const resolutionWarning=$derived(panelResolutionWarning(panel?.document.presentation.viewports.find(viewport=>viewport.pageId===displayedPage),displayWidth,displayHeight,pixelRatio));
  const resolutionKey=$derived(`${panel?.publicationHash}:${pageId}:${resolutionWarning}`);
  let dismissedResolution=$state('');
  let panelView=$state<{reset:()=>void;suspend:()=>void;enableAudio:()=>Promise<void>}>();
  let panelSurfaces=$state.raw<PreparedCompiledSurfaces>(),viewRevision=$state(0);
  let preparing:AbortController|undefined;
  let muted=$state(false);
  let audioAvailability=$state<AudioAvailability>('locked');
  let cache=createCompiledAssetCache(browserCompiledCacheStore());
  let assignments=$state<{instanceId:string;publicationHash:string;name:string}[]>([]), selected=$state('');
  let panelError=$state(false);
  let active=$state(false), name=$state('Browser companion'), secret=$state(''), fullscreen=$state(false);
  let hasFocus=$state(true), inputReady=$state(false);
  let feedback=$state<FeedbackSamples>({}), feedbackNotice=$state('');
  let suppressFeedbackCues=$state(true);
  const feedbackView=createBrowserFeedback();
  let installPrompt=$state<any>(null), installHelp=$state(false), owner=$state(false);
  let connection:BrowserTransport|null=null, epoch=0, sequence=0, heartbeat=0, resyncSequence=0; let session=$state.raw<Record<string,any>|null>(null);
  let connecting=$state(false);
  let disposed=false, pageHidden=false, nextAttempt=0, lastTick=performance.now();
  let handlingRuntimeError=false;
  let focusWork:Promise<void>|undefined;
  let preparationHeartbeatTimer:ReturnType<typeof setInterval>|undefined;
  let staleRecoveryWork:Promise<void>|undefined;
  const obsoleteSession=(reason:unknown)=>String(reason).includes('Session is obsolete');
  const visible=()=>!pageHidden&&document.visibilityState==='visible';
  const focused=()=>visible()&&hasFocus;
  const current=(socket:BrowserTransport,version:number)=>version===epoch&&!disposed&&owner&&!pageHidden&&connection===socket;
  function emitOutput(change:{controlId:string;value:boolean|number}){if(active&&inputReady&&focused())queue.enqueue(epoch,[change]);}
  function recoverObsoleteSession(reason:unknown,socket=connection,version=epoch):boolean{
    if(!obsoleteSession(reason))return false;
    error='';feedbackNotice='';
    if(!socket){if(!connection){stop('Reconnecting to Cockpit…');nextAttempt=0;}return true;}
    if(version!==epoch||connection!==socket||disposed||pageHidden||!owner)return true;
    if(staleRecoveryWork)return true;
    if(!session||!panel){stop('Reconnecting to Cockpit…');nextAttempt=0;return true;}
    staleRecoveryWork=Promise.resolve().then(async()=>{
      queue.stop();active=false;inputReady=false;panelError=false;panelView?.suspend();session=null;
      feedbackView.disconnect();feedback=feedbackView.snapshot();suppressFeedbackCues=true;
      const presence=await socket.request({type:'presence',display:panelDisplayReport(panel?.publicationHash,displayedPage,panel?displayWidth:windowWidth,panel?displayHeight:windowHeight,pixelRatio)});
      if(!current(socket,version))return;
      await loadAssigned(socket,version,presence.assignments);
    }).catch(e=>{
      if(!current(socket,version))return;
      if(obsoleteSession(e)){error='';stop('Reconnecting to Cockpit…');nextAttempt=performance.now()+1000;}
      else{error=String(e instanceof Error?e.message:e);stop('Offline · reconnecting to Cockpit');}
    }).finally(()=>{staleRecoveryWork=undefined;});
    return true;
  }
  async function availableAssets(){try{return await cache.haveAssets();}catch(e){warning='Compiled asset cache unavailable; receiving fresh content.';let stored:string|undefined;cache=createCompiledAssetCache({read:async()=>stored,write:async value=>{stored=value;}});return [];}}
  const queue=createOrderedInputQueue(async(version,changes)=>{
    if(version!==epoch || !active || !inputReady || !connection || !session || !focused())throw new Error('Input session stopped');
    await connection.request(frame('input',{sequence:String(++sequence),changes}));
  },reason=>{if(!recoverObsoleteSession(reason))stop(reason);});
  function frame(type:string,rest:Record<string,unknown>={}) {
    if(!session)throw new Error('No active panel session');
    return {version:1,type,clientId:session.clientId,sessionId:session.sessionId,generation:session.generation,publicationHash:session.publicationHash,...rest};
  }
  function stop(reason:string) {
    epoch++;preparing?.abort();preparing=undefined;if(preparationHeartbeatTimer)clearInterval(preparationHeartbeatTimer);preparationHeartbeatTimer=undefined;active=false;inputReady=false;panelError=false;queue.stop();panelView?.suspend();session=null;
    feedbackView.disconnect();feedback=feedbackView.snapshot();suppressFeedbackCues=true;
    const previous=connection;connection=null;previous?.close();status=reason;nextAttempt=performance.now()+500;
  }
  function runtimeError(message:string){
    error=message;
    if(handlingRuntimeError)return;
    handlingRuntimeError=true;
    try{stop('Compiled player error · controls released');nextAttempt=performance.now()+2000;}
    finally{handlingRuntimeError=false;}
  }
  function consume(reply:Record<string,any>,version:number){
    if(version!==epoch||disposed||!session||!panel||!reply.feedback)return;
    if(feedbackView.accept(session as FeedbackIdentity,reply.feedback,panel.document.ui,performance.now(),reply.type==='feedback-resync')){suppressFeedbackCues=reply.feedback.mode==='snapshot'||reply.type==='feedback-resync';feedback=feedbackView.snapshot();}
  }
  function resetFeedback(){feedbackView.reset();feedback={};suppressFeedbackCues=true;feedbackNotice='Visual state cleared locally. Controls are unchanged.';}
  async function resyncFeedback(){
    if(!connection||!session||!active)return;
    const socket=connection,version=epoch;
    try{await socket.request(frame('feedback-resync',{sequence:String(++resyncSequence)}));if(version===epoch)feedbackNotice='Fresh visual snapshot requested; controls were not replayed.';}
    catch(e){if(version===epoch){if(!recoverObsoleteSession(e,socket,version)){feedbackNotice=String(e);stop('Feedback resync unavailable');}}}
  }
  async function loadAssigned(socket:BrowserTransport, version:number, rows:typeof assignments){
    if(!current(socket,version))return;
    active=false;queue.stop();panelView?.suspend();
    assignments=rows;
    if(!rows.some(a=>a.instanceId===selected))selected=rows[0]?.instanceId??'';
    const assignment=rows.find(a=>a.instanceId===selected);
    if(!assignment || !visible()){status=assignment?'Connected · panel paused while inactive':'Connected · assign a panel in Cockpit';return;}
    const haveAssets=await availableAssets();if(!current(socket,version))return;
    const opened=await socket.request({type:'open',instanceId:assignment.instanceId,publicationHash:assignment.publicationHash,widgetCapabilities:BROWSER_WIDGET_CAPABILITIES,haveAssets});
    if(!current(socket,version))return;
    const p=opened.publication;
    let verified,candidate:PreparedCompiledSurfaces|undefined,preparationHeartbeatWork:Promise<void>|undefined;
    const controller=new AbortController(),preparationOperation={instanceId:assignment.instanceId,publicationHash:opened.publicationHash,sessionId:opened.sessionId,generation:opened.generation,attemptId:'1',phase:'checking-cache'};preparing=controller;heartbeat=0;
    const sendPreparationHeartbeat=()=>{
      if(!current(socket,version)||controller.signal.aborted||preparationHeartbeatWork)return;
      preparationHeartbeatWork=socket.request({version:1,type:'heartbeat',clientId:opened.clientId,sessionId:opened.sessionId,generation:opened.generation,publicationHash:opened.publicationHash,sequence:String(++heartbeat),operation:preparationOperation})
        .then(()=>{}).catch(e=>{if(current(socket,version)){if(!recoverObsoleteSession(e,socket,version)){error=String(e instanceof Error?e.message:e);stop('Offline · reconnecting to Cockpit');}}})
        .finally(()=>preparationHeartbeatWork=undefined);
    };
    const loadHeartbeatTimer=setInterval(sendPreparationHeartbeat,500);
    preparationHeartbeatTimer=loadHeartbeatTimer;
    sendPreparationHeartbeat();
    try{
      verified=await cache.verifyDelivery(p);if(!current(socket,version))return;
      if(opened.publicationHash!==assignment.publicationHash || verified.publicationHash!==assignment.publicationHash)throw new Error('Panel assignment changed during delivery');
      candidate=await prepareCompiledSurfaces(verified,{signal:controller.signal});
      if(preparationHeartbeatWork)await preparationHeartbeatWork;
      if(!current(socket,version)){disposeCompiledSurfaces(candidate);candidate=undefined;return;}
    }
    catch(e){
      if(!current(socket,version)){disposeCompiledSurfaces(candidate);candidate=undefined;return;}
      session=opened;panelError=true;active=false;
      panelView?.suspend();queue.stop();feedbackView.clear();feedback={};
      status='Connected · panel error';error=obsoleteSession(e)?'':String(e instanceof Error?e.message:e);return;
    }finally{if(preparationHeartbeatTimer===loadHeartbeatTimer){clearInterval(loadHeartbeatTimer);preparationHeartbeatTimer=undefined;}if(preparing===controller)preparing=undefined;}
    if(!current(socket,version)){disposeCompiledSurfaces(candidate);return;}
    const previousSurfaces=panelSurfaces;panelSurfaces=candidate;viewRevision++;
    panel=verified;pageId=panel.document.ui.pages.some(p=>p.id===pageId)?pageId:panel.document.ui.pages[0].id;
    void cache.save(verified).catch(()=>warning='Compiled visuals are active; durable cache storage is unavailable.');

    panelError=false;session=opened;sequence=0;resyncSequence=0;feedbackView.clear();feedback={};suppressFeedbackCues=true;feedbackNotice='';
    await tick();disposeCompiledSurfaces(previousSurfaces);if(!current(socket,version))return;
    const values=Object.fromEntries(panel.document.ui.controls.map(control=>[control.id,control.kind==='button'?false:control.fallback]));
    await socket.request(frame('baseline',{sequence:'0',values}));
    if(!current(socket,version))return;
    inputReady=focused();if(inputReady)queue.start();else queue.stop();
    active=true;status=focused()?'Connected · panel active':'Connected · feedback active · controls paused';error='';
  }
  async function connect(){
    if(!owner || connecting || connection || !credential || disposed || pageHidden || performance.now()<nextAttempt)return;
    connecting=true;const savedCredential=credential;const version=epoch,socket=new BrowserTransport();connection=socket;
    socket.onreply=reply=>consume(reply,version);
    socket.onclose=()=>{if(connection===socket)stop('Offline · reconnecting to Cockpit');};
    try {
      await socket.connect();if(!current(socket,version))return;
      const auth=await socket.request({type:'auth',token:savedCredential.token});
      if(!current(socket,version))return;
      if(auth.hostId!==savedCredential.hostId || auth.companionId!==savedCredential.companionId)throw new Error('Saved Cockpit identity differs');
      await loadAssigned(socket,version,auth.assignments);
    }catch(e){if(version===epoch&&!recoverObsoleteSession(e,socket,version)){error=String(e instanceof Error?e.message:e);stop('Offline · reconnecting to Cockpit');nextAttempt=performance.now()+2000;
      if(error.includes('invalid or revoked')){credential=null;clearCredential(localStorage,savedCredential.token);status='Pair this browser again in Cockpit';}
    }}
    finally{connecting=false;if(!credential && secret)void pair();}
  }
  async function pair(){
    if(!owner || connecting || !secret || disposed || !visible())return;
    connecting=true;error='';stop('Pairing with Cockpit…');const version=epoch,socket=new BrowserTransport();connection=socket;
    try{
      // Check storage before consuming the one-use offer.
      localStorage.setItem('osg-storage-probe','ok');localStorage.removeItem('osg-storage-probe');
      await socket.connect();
      if(!current(socket,version))return;
      const companionId=crypto.randomUUID();
      const previous=credential;
      const pending=await socket.request({type:'pair',secret,companionId,name:name.trim(),...(previous?{replaceToken:previous.token}:{})});
      if(!current(socket,version))return;
      if(previous){
        clearCredential(localStorage,previous.token);credential=null;panel=null;assignments=[];
        warning='The previous pairing was removed from this browser and Cockpit. This browser is being paired again.';
      }
      const paired=await socket.request({type:'pair-status',requestId:pending.requestId,secret});
      if(!current(socket,version))return;
      if(paired.type!=='paired')throw new Error('Pairing was not approved. Start again in Cockpit.');
      const saved={token:paired.token,companionId:paired.companionId,hostId:paired.hostId};
      saveCredential(localStorage,saved);credential=saved;secret='';history.replaceState(null,'','/');
      if(version===epoch){stop('Pairing saved · connecting…');nextAttempt=0;}
    }catch(e){if(version===epoch&&!disposed){secret='';error=obsoleteSession(e)?'':String(e instanceof Error?e.message:e);stop('Pairing did not complete');}}
    finally{connecting=false;void connect();}
  }
  let polling=false;
  async function poll(){
    const now=performance.now();
    if(now-lastTick>1000 && active&&inputReady)void refreshControls(true);lastTick=now;
    if(polling || connecting || disposed || pageHidden || focusWork || staleRecoveryWork)return;
    if(!connection){void connect();return;}
    const socket=connection,version=epoch;polling=true;
    try{
      const presence=await socket.request({type:'presence',display:panelDisplayReport(panel?.publicationHash,displayedPage,panel?displayWidth:windowWidth,panel?displayHeight:windowHeight,pixelRatio)});
      if(version!==epoch)return;
      if(focusWork||staleRecoveryWork)return;
      if(JSON.stringify(presence.assignments)!==JSON.stringify(assignments)){stop('Panel assignment updated');nextAttempt=0;return;}
      if(session && (active||panelError))await socket.request(frame('heartbeat',{sequence:String(++heartbeat),...(panelError?{panelError:true}:{})}));
      else if(!session && visible() && assignments.length)await loadAssigned(socket,version,assignments);
    }catch(e){if(version===epoch&&!recoverObsoleteSession(e,socket,version)){error=String(e instanceof Error?e.message:e);stop('Offline · reconnecting to Cockpit');}}
    finally{polling=false;}
  }
  async function toggleFullscreen(){
    try{if(document.fullscreenElement)await document.exitFullscreen();else await document.documentElement.requestFullscreen();}
    catch{error='Fullscreen is unavailable in this browser. You can use the browser fullscreen command instead.';}
  }
  async function install(){
    if(!installPrompt){installHelp=true;return;}
    await installPrompt.prompt();await installPrompt.userChoice;installPrompt=null;
  }
  function changedPage(value:string){stop('Panel page changed');pageId=value;nextAttempt=0;}
  function resumeVisible(){if(!visible()||disposed)return;nextAttempt=0;return connect();}
  function refreshControls(reopen=false):Promise<void>|undefined {
    if(staleRecoveryWork)return staleRecoveryWork;
    queue.stop();inputReady=false;panelView?.reset();
    if(focusWork)return focusWork;
    if(!connection||!session||!panel)return resumeVisible();
    const socket=connection,version=epoch;
    focusWork=Promise.resolve().then(async()=>{
      try{
        if(!reopen){
          const changes=panel!.document.ui.controls.map(control=>({controlId:control.id,value:control.kind==='button'?false:control.fallback}));
          for(let offset=0;offset<changes.length;offset+=64)await socket.request(frame('input',{sequence:String(++sequence),changes:changes.slice(offset,offset+64)}));
          if(!current(socket,version))return;
          status='Connected · feedback active · controls paused';
        }
        if(focused()||reopen){
          const close=frame('close');session=null;active=false;
          try{await socket.request(close);}catch(e){if(!String(e).includes('Session is obsolete'))throw e;}
          if(current(socket,version))await loadAssigned(socket,version,assignments);
        }
      }catch(e){if(current(socket,version)&&!recoverObsoleteSession(e,socket,version)){error=String(e);stop('Controls released · reconnecting to Cockpit');}}
    }).finally(()=>{focusWork=undefined;});
    return focusWork;
  }
  function blurred(){hasFocus=false;return refreshControls();}
  function gainedFocus(){hasFocus=document.hasFocus();if(focused()&&!disposed)return refreshControls(true);}
  function visibilityChanged(){
    if(!visible()){if(secret){secret='';stop('Pairing paused while hidden');}else return blurred();}
    else{hasFocus=document.hasFocus();return refreshControls(true);}
  }
  function pageHiding(){pageHidden=true;secret='';stop('Panel page unloaded');}
  function pageShowing(){pageHidden=false;return resumeVisible();}
  onMount(()=>{
    hasFocus=document.hasFocus();
    try{credential=readCredential(localStorage);}catch(e){error=String(e);}
    secret=new URLSearchParams(location.hash.slice(1)).get('pair')??'';
    history.replaceState(null,'','/');
    if(secret && !/^[A-Za-z0-9_-]{43}$/.test(secret)){secret='';error='Pairing link is invalid. Open a new browser pairing from Cockpit.';}

    if(secret && credential)warning='This browser is already paired. Opening this link replaces its previous pairing in both the browser and Cockpit.';
    const storageChanged=(event:StorageEvent)=>{
      if(!['osg-browser-credential','osg-browser-credential-v2'].includes(event.key??'') || !credential)return;
      let saved:Credential|null=null;try{saved=readCredential(localStorage);}catch{}
      if(saved?.token===credential.token)return;
      stop('Pairing replaced in another browser window');credential=null;panel=null;assignments=[];owner=false;releaseOwner();
      warning='This browser pairing was replaced in another window. Reopen the companion to use the new pairing.';
    };
    window.addEventListener('storage',storageChanged);
    const full=()=>fullscreen=!!document.fullscreenElement;
    const beforeInstall=(event:Event)=>{event.preventDefault();installPrompt=event;};
    window.addEventListener('blur',blurred);window.addEventListener('pagehide',pageHiding);window.addEventListener('pageshow',pageShowing);window.addEventListener('focus',gainedFocus);
    document.addEventListener('visibilitychange',visibilityChanged);document.addEventListener('fullscreenchange',full);window.addEventListener('beforeinstallprompt',beforeInstall);
    const timer=setInterval(()=>{if(owner)void poll();},250);
    let releaseOwner:()=>void=()=>{};
    const initialize=()=>{owner=true;if(secret)void pair();else void connect();};
    if(navigator.locks)void navigator.locks.request('osg-companion-owner',secret?{steal:true}:{ifAvailable:true},async lock=>{
      if(!lock){status='Companion is already open in another browser window';return;}
      if(disposed)return;
      initialize();await new Promise<void>(resolve=>releaseOwner=resolve);
    }).catch(e=>error=String(e));
    else initialize();
    return()=>{disposed=true;window.removeEventListener('storage',storageChanged);releaseOwner();stop('Browser closed');disposeCompiledSurfaces(panelSurfaces);clearInterval(timer);window.removeEventListener('blur',blurred);window.removeEventListener('pagehide',pageHiding);window.removeEventListener('pageshow',pageShowing);window.removeEventListener('focus',gainedFocus);document.removeEventListener('visibilitychange',visibilityChanged);document.removeEventListener('fullscreenchange',full);window.removeEventListener('beforeinstallprompt',beforeInstall);};
  });
</script>

<svelte:window bind:devicePixelRatio={pixelRatio} bind:innerWidth={windowWidth} bind:innerHeight={windowHeight}/>
<div class="shell" class:fullscreen>
  <header><strong>OpenSimGear <span>Companion</span></strong><div><button onclick={install}>↓ Install app</button><button onclick={toggleFullscreen}>⛶ {fullscreen?'Exit fullscreen':'Fullscreen'}</button></div></header>
  {#if installHelp}<p class="help">Use your browser menu → Install app or Add to home screen. If installation is unavailable, bookmark this page for quick access. Keep using this browser profile to retain pairing.</p>{/if}
  {#if !credential}
    <section class="pair"><h1>Browser companion</h1>
      {#if !owner}<p>{status}</p>{/if}
      {#if secret}<p>Pairing this browser with Cockpit automatically. Pairing is remembered in this browser profile.</p><label>Companion name<input bind:value={name} maxlength="80" /></label><button disabled={connecting||!name.trim()} onclick={pair}>⇄ Pair with Cockpit</button>
      {:else}<p>In Cockpit, choose Companions → Pair → Browser and open the pairing link.</p>{/if}
    </section>
  {:else}
    <div class="toolbar"><span class:active={active||panelError}>{status}</span>
      {#if assignments.length>1}<label>Panel<select value={selected} onchange={e=>{stop('Switching panel');selected=e.currentTarget.value;nextAttempt=0;}}>{#each assignments as a}<option value={a.instanceId}>{a.name}</option>{/each}</select></label>{/if}
      {#if panel && panel.document.ui.pages.length>1}<label>Page<select value={pageId} onchange={e=>changedPage(e.currentTarget.value)}>{#each panel.document.ui.pages as page}<option value={page.id}>{page.title}</option>{/each}</select></label>{/if}
    </div>
    {#if panel}<main class:inactive={!active} bind:clientWidth={displayWidth} bind:clientHeight={displayHeight}>{#key viewRevision}<CompiledPanel bind:this={panelView} publication={panel} prepared={panelSurfaces} {pageId} {feedback} {suppressFeedbackCues} {muted} interactive={active&&inputReady&&focused()} onoutput={emitOutput} onerror={runtimeError} onaudio={value=>audioAvailability=value}/>{/key}</main>
      <div class="toolbar"><span>Visuals: requested, host accepted, host submitted and game confirmed are separate states.</span><button onclick={resetFeedback}>↻ Clear visual state</button>{#if panel.document.presentation.artifacts.some(entry=>entry.artifact.sounds.length>0)}<label><input type="checkbox" bind:checked={muted}/>Mute</label><button onclick={()=>void panelView?.enableAudio()} disabled={audioAvailability==="unsupported"||audioAvailability==="muted"}>♪ Enable sound</button><span role="status">Audio: {audioAvailability}</span>{/if}<button onclick={resyncFeedback} disabled={!active}>↻ Resync visuals</button>{#if feedbackNotice}<span role="status">{feedbackNotice}</span>{/if}</div>
    {:else}<section class="empty"><h1>{status}</h1><p>{panelError?'Edit and save this panel in Cockpit to retry.':'Choose a panel for this companion in Cockpit. It will appear here automatically.'}</p></section>{/if}
  {/if}
  {#if panel && resolutionWarning && dismissedResolution!==resolutionKey}<aside class="resolution-warning" role="status">{resolutionWarning}<button aria-label="Dismiss resolution warning" onclick={()=>dismissedResolution=resolutionKey}>×</button></aside>{/if}
  {#if warning && !error}<aside role="alert">{warning}<button aria-label="Dismiss notification" onclick={()=>warning=''}>×</button></aside>{/if}
  {#if error}<aside role="alert">{error}<button aria-label="Dismiss notification" onclick={()=>error=''}>×</button></aside>{/if}
</div>
<style>
  :global(body){margin:0;background:#111a22;color:#e1eaf0;font-family:system-ui,sans-serif}:global(*){box-sizing:border-box}.shell{min-height:100dvh;padding:20px}header,.toolbar,header>div{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap}header{padding-bottom:20px;border-bottom:1px solid #344452}header span{color:#64caba}button,input,select{font:inherit;border:1px solid #344452;border-radius:7px;padding:10px 14px;background:#1a2935;color:#e1eaf0}button{cursor:pointer}button:disabled{opacity:.5;cursor:default}button:hover{border-color:#64caba}label{display:flex;align-items:center;gap:10px}.pair,.empty{max-width:620px;margin:60px auto;padding:25px;border:1px solid #344452;border-radius:10px}.pair label{margin:25px 0}.toolbar{padding:16px 0;font-size:14px;color:#9aafbf}.toolbar .active{color:#64caba}main{max-width:1600px;margin:auto}.inactive{opacity:.55;pointer-events:none}.help{color:#9aafbf;font-size:14px;line-height:1.6}aside{position:fixed;right:20px;bottom:20px;max-width:500px;padding:16px;border:1px solid #e9b75c;background:#1a2935;border-radius:8px;box-shadow:0 8px 30px #0008;display:flex;align-items:center;gap:12px;z-index:100}aside button{padding:3px 9px}
  .resolution-warning{left:20px;right:auto;max-width:min(500px,calc(100vw - 40px));font-size:14px;line-height:1.5}.resolution-warning button{flex-shrink:0}
  .shell.fullscreen{height:100dvh;min-height:0;padding:0;overflow:hidden}.shell.fullscreen>:not(main):not(.resolution-warning){display:none}.shell.fullscreen main{width:100%;height:100%;max-width:none;margin:0}.shell.fullscreen main :global(.compiled-panel){height:100%;min-height:0;border:0;border-radius:0;aspect-ratio:auto!important}
</style>
