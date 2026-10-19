<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { PanelCanvas, createPreviewController, fallbackValue, type FeedbackSamples } from '@opensimgear/companion-ui';
  import { flightTemplate, type DesignerDocument } from '@opensimgear/companion-designer';
  import { verifyPublishedPayload } from '../../companion/src/lib/companion-cache';
  import { createOrderedInputQueue } from '../../companion/src/lib/input-queue';
  import { BrowserTransport, readCredential, saveCredential, clearCredential, type Credential } from './transport';
  import { createBrowserFeedback, type FeedbackIdentity } from './feedback';
  let credential=$state<Credential|null>(null), error=$state(''), warning=$state(''), status=$state('Connecting…');
  let panel=$state<DesignerDocument|null>(null), pageId=$state(''), assets=$state<Record<string,string>>({});
  let assignments=$state<{instanceId:string;publicationHash:string;name:string}[]>([]), selected=$state('');
  let panelError=$state(false);
  let active=$state(false), name=$state('Browser companion'), secret=$state(''), fullscreen=$state(false);
  let feedback=$state<FeedbackSamples>({}), feedbackNotice=$state('');
  const feedbackView=createBrowserFeedback();
  let installPrompt=$state<any>(null), installHelp=$state(false), owner=$state(false);
  let connection:BrowserTransport|null=null, epoch=0, session:Record<string,any>|null=null, sequence=0, heartbeat=0, resyncSequence=0;
  let connecting=$state(false);
  let disposed=false, pageHidden=false, nextAttempt=0, lastTick=performance.now();
  const visible=()=>!pageHidden&&document.visibilityState==='visible';
  const focused=()=>visible()&&document.hasFocus();
  const current=(socket:BrowserTransport,version:number)=>version===epoch&&!disposed&&owner&&visible()&&connection===socket;
  const controller=createPreviewController(flightTemplate,{onchange:(changes)=>{if(active&&focused())queue.enqueue(epoch,changes);}});
  const queue=createOrderedInputQueue(async(version,changes)=>{
    if(version!==epoch || !active || !connection || !session || !focused())throw new Error('Input session stopped');
    await connection.request(frame('input',{sequence:String(++sequence),changes}));
  },reason=>stop(reason));
  function frame(type:string,rest:Record<string,unknown>={}) {
    if(!session)throw new Error('No active panel session');
    return {version:1,type,clientId:session.clientId,sessionId:session.sessionId,generation:session.generation,publicationHash:session.publicationHash,...rest};
  }
  function stop(reason:string) {
    epoch++; active=false;panelError=false;queue.stop();controller.disarm(reason);session=null;
    feedbackView.disconnect();feedback=feedbackView.snapshot();
    const previous=connection;connection=null;previous?.close();status=reason;nextAttempt=performance.now()+500;
  }
  function consume(reply:Record<string,any>,version:number){
    if(version!==epoch||disposed||!visible()||!session||!panel||!reply.feedback)return;
    if(feedbackView.accept(session as FeedbackIdentity,reply.feedback,panel.ui,performance.now(),reply.type==='feedback-resync'))feedback=feedbackView.snapshot();
  }
  function resetFeedback(){feedbackView.reset();feedback={};feedbackNotice='Visual state cleared locally. Controls are unchanged.';}
  async function resyncFeedback(){
    if(!connection||!session||!active)return;
    const version=epoch;
    try{await connection.request(frame('feedback-resync',{sequence:String(++resyncSequence)}));if(version===epoch)feedbackNotice='Fresh visual snapshot requested; controls were not replayed.';}
    catch(e){if(version===epoch){feedbackNotice=String(e);stop('Feedback resync unavailable');}}
  }
  async function loadAssigned(socket:BrowserTransport, version:number, rows:typeof assignments){
    if(!current(socket,version))return;
    assignments=rows;
    if(!rows.some(a=>a.instanceId===selected))selected=rows[0]?.instanceId??'';
    const assignment=rows.find(a=>a.instanceId===selected);
    if(!assignment || !visible()){status=assignment?'Connected · panel paused while inactive':'Connected · assign a panel in Cockpit';return;}
    const opened=await socket.request({type:'open',instanceId:assignment.instanceId,publicationHash:assignment.publicationHash});
    const p=opened.publication;
    let verified;
    try{verified=await verifyPublishedPayload({...p,document:JSON.parse(p.canonicalDocument)});}
    catch(e){
      if(!current(socket,version))return;
      session=opened;heartbeat=0;panel=null;assets={};panelError=true;active=false;
      controller.disarm('Panel error');queue.stop();feedbackView.clear();feedback={};
      status='Connected · panel error';error=String(e instanceof Error?e.message:e);return;
    }
    if(!current(socket,version))return;
    if(opened.publicationHash!==assignment.publicationHash || verified.publicationHash!==assignment.publicationHash)throw new Error('Panel assignment changed during delivery');
    panel=verified.document;pageId=panel.ui.pages.some(p=>p.id===pageId)?pageId:panel.ui.pages[0].id;
    assets=Object.fromEntries(verified.assets.map(a=>[a.assetId,'data:'+panel!.ui.assets.find(m=>m.id===a.assetId)!.mime+';base64,'+a.base64]));
    controller.setDocument(panel);
    panelError=false;session=opened;sequence=0;heartbeat=0;resyncSequence=0;feedbackView.clear();feedback={};feedbackNotice='';
    await tick();if(!current(socket,version))return;
    const values=Object.fromEntries(panel.ui.controls.map(c=>[c.id,fallbackValue(c)]));
    await socket.request(frame('baseline',{sequence:'0',values}));
    if(!current(socket,version))return;
    if(focused()){controller.arm();queue.start();}
    else{controller.disarm('Controls paused while unfocused');queue.stop();}
    active=true;status=focused()?'Connected · panel active':'Connected · feedback active · controls paused';error='';
  }
  async function connect(){
    if(!owner || connecting || connection || !credential || disposed || !visible() || performance.now()<nextAttempt)return;
    connecting=true;const savedCredential=credential;const version=epoch,socket=new BrowserTransport();connection=socket;
    socket.onreply=reply=>consume(reply,version);
    socket.onclose=()=>{if(connection===socket)stop('Offline · reconnecting to Cockpit');};
    try {
      await socket.connect();if(!current(socket,version))return;
      const auth=await socket.request({type:'auth',token:savedCredential.token});
      if(!current(socket,version))return;
      if(auth.hostId!==savedCredential.hostId || auth.companionId!==savedCredential.companionId)throw new Error('Saved Cockpit identity differs');
      await loadAssigned(socket,version,auth.assignments);
    }catch(e){if(version===epoch){error=String(e instanceof Error?e.message:e);stop('Offline · reconnecting to Cockpit');nextAttempt=performance.now()+2000;
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
    }catch(e){if(version===epoch&&!disposed){secret='';error=String(e instanceof Error?e.message:e);stop('Pairing did not complete');}}
    finally{connecting=false;void connect();}
  }
  let polling=false;
  async function poll(){
    const now=performance.now();
    if(now-lastTick>1000 && active)stop('Panel paused after timing gap');lastTick=now;
    if(polling || connecting || disposed)return;
    if(!visible()){if(connection||active)stop('Panel paused while hidden');return;}
    if(!connection){void connect();return;}
    const socket=connection,version=epoch;polling=true;
    try{
      const presence=await socket.request({type:'presence'});
      if(version!==epoch)return;
      if(JSON.stringify(presence.assignments)!==JSON.stringify(assignments)){stop('Panel assignment updated');nextAttempt=0;return;}
      if(session && (active||panelError) && visible())await socket.request(frame('heartbeat',{sequence:String(++heartbeat),...(panelError?{panelError:true}:{})}));
      else if(!session && visible() && assignments.length){stop('Resuming panel');nextAttempt=0;}
    }catch(e){if(version===epoch){error=String(e instanceof Error?e.message:e);stop('Offline · reconnecting to Cockpit');}}
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
  function blurred(){
    // End every old input job/socket before establishing a neutral background
    // session. Pairing/authentication without an input session can finish.
    if(session||active)stop('Controls released · reconnecting visuals');
    return resumeVisible();
  }
  function gainedFocus(){
    if(!focused()||disposed)return;
    if(session||active)stop('Refreshing focused control baseline');
    return resumeVisible();
  }
  function visibilityChanged(){if(!visible()){secret='';stop('Panel paused while hidden');}else return resumeVisible();}
  function pageHiding(){pageHidden=true;secret='';stop('Panel page unloaded');}
  function pageShowing(){pageHidden=false;return resumeVisible();}
  onMount(()=>{
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
    return()=>{disposed=true;window.removeEventListener('storage',storageChanged);releaseOwner();stop('Browser closed');clearInterval(timer);window.removeEventListener('blur',blurred);window.removeEventListener('pagehide',pageHiding);window.removeEventListener('pageshow',pageShowing);window.removeEventListener('focus',gainedFocus);document.removeEventListener('visibilitychange',visibilityChanged);document.removeEventListener('fullscreenchange',full);window.removeEventListener('beforeinstallprompt',beforeInstall);};
  });
</script>

<div class="shell" class:fullscreen>
  <header><strong>OpenSimGear <span>Companion</span></strong><div><button onclick={install}>Install app</button><button onclick={toggleFullscreen}>{fullscreen?'Exit fullscreen':'Fullscreen'}</button></div></header>
  {#if installHelp}<p class="help">Use your browser menu → Install app or Add to home screen. If installation is unavailable, bookmark this page for quick access. Keep using this browser profile to retain pairing.</p>{/if}
  {#if !credential}
    <section class="pair"><h1>Browser companion</h1>
      {#if !owner}<p>{status}</p>{/if}
      {#if secret}<p>Pairing this browser with Cockpit automatically. Pairing is remembered in this browser profile.</p><label>Companion name<input bind:value={name} maxlength="80" /></label><button disabled={connecting||!name.trim()} onclick={pair}>Pair with Cockpit</button>
      {:else}<p>In Cockpit, choose Companions → Pair → Browser and open the pairing link.</p>{/if}
    </section>
  {:else}
    <div class="toolbar"><span class:active={active||panelError}>{status}</span>
      {#if assignments.length>1}<label>Panel<select value={selected} onchange={e=>{stop('Switching panel');selected=e.currentTarget.value;nextAttempt=0;}}>{#each assignments as a}<option value={a.instanceId}>{a.name}</option>{/each}</select></label>{/if}
      {#if panel && panel.ui.pages.length>1}<label>Page<select value={pageId} onchange={e=>changedPage(e.currentTarget.value)}>{#each panel.ui.pages as page}<option value={page.id}>{page.title}</option>{/each}</select></label>{/if}
    </div>
    {#if panel}<main class:inactive={!active}><PanelCanvas document={panel} {pageId} {assets} {controller} {feedback} /></main>
      <div class="toolbar"><span>Visuals: requested, host accepted, host submitted and game confirmed are separate states.</span><button onclick={resetFeedback}>Clear visual state</button><button onclick={resyncFeedback} disabled={!active}>Resync visuals</button>{#if feedbackNotice}<span role="status">{feedbackNotice}</span>{/if}</div>
    {:else}<section class="empty"><h1>{status}</h1><p>{panelError?'Edit and save this panel in Cockpit to retry.':'Choose a panel for this companion in Cockpit. It will appear here automatically.'}</p></section>{/if}
  {/if}
  {#if warning && !error}<aside role="alert">{warning}<button aria-label="Dismiss notification" onclick={()=>warning=''}>×</button></aside>{/if}
  {#if error}<aside role="alert">{error}<button aria-label="Dismiss notification" onclick={()=>error=''}>×</button></aside>{/if}
</div>
<style>
  :global(body){margin:0;background:#111a22;color:#e1eaf0;font-family:system-ui,sans-serif}:global(*){box-sizing:border-box}.shell{min-height:100dvh;padding:20px}header,.toolbar,header>div{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap}header{padding-bottom:20px;border-bottom:1px solid #344452}header span{color:#64caba}button,input,select{font:inherit;border:1px solid #344452;border-radius:7px;padding:10px 14px;background:#1a2935;color:#e1eaf0}button{cursor:pointer}button:disabled{opacity:.5;cursor:default}button:hover{border-color:#64caba}label{display:flex;align-items:center;gap:10px}.pair,.empty{max-width:620px;margin:60px auto;padding:25px;border:1px solid #344452;border-radius:10px}.pair label{margin:25px 0}.toolbar{padding:16px 0;font-size:14px;color:#9aafbf}.toolbar .active{color:#64caba}main{max-width:1600px;margin:auto}.inactive{opacity:.55;pointer-events:none}.help{color:#9aafbf;font-size:14px;line-height:1.6}aside{position:fixed;right:20px;bottom:20px;max-width:500px;padding:16px;border:1px solid #e9b75c;background:#1a2935;border-radius:8px;box-shadow:0 8px 30px #0008;display:flex;align-items:center;gap:12px;z-index:100}aside button{padding:3px 9px}
  .shell.fullscreen{height:100dvh;min-height:0;padding:0;overflow:hidden}.shell.fullscreen>:not(main){display:none}.shell.fullscreen main{width:100%;height:100%;max-width:none;margin:0}.shell.fullscreen main :global(.panel-canvas){height:100%;min-height:0;border:0;border-radius:0;aspect-ratio:auto!important}
</style>
