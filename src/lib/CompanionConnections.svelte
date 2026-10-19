<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import Notification from './Notification.svelte';
  import { newPairingConnected } from './companion-pairing';
  import { invoke } from '@tauri-apps/api/core';
  import Esp32Companions from './Esp32Companions.svelte';
  import { gatewayRequest } from './gateway';
  import { notify } from './notifications';
  import { sendPublication, type CompanionCommand, type CompanionHostState } from './companions';
  import type { Publication } from '@opensimgear/companion-designer';
  let { publications = [], openSettings, initialCompanionId = '', onViewChange }: { publications?: Publication[]; openSettings: () => void; initialCompanionId?: string; onViewChange?: (detailsOpen: boolean) => void } = $props();
  let hostState = $state.raw<CompanionHostState>({});
  let pendingCompanionId = $state(untrack(() => initialCompanionId));
  let disposed = false;
  let pairingOpen = $state(false);
  let browserOfferAttempted=$state(false);
  $effect(()=>{
    if(pairingOpen && kind==='browser' && hostState.listener?.enabled && !pairing && !busy && !browserOfferAttempted){
      browserOfferAttempted=true;void action(offer);
    }
  });
  let kind = $state<'' | 'browser' | 'mobile' | 'esp32'>('');
  let busy = $state(false);
  let selectedId=$state(''), editedName=$state(''), editedPanel=$state('');
  let pairing = $state<CompanionHostState['pairing']>();
  let secondsLeft = $state(0), expires = 0;
  let baseline = new Set<string>();
  let polling = false;
  let updateOpen=$state(false), updatePort=$state(''), updateProfile=$state(''), updating=$state(false);
  const firmwareInfo=(client:Record<string,unknown>)=>hostState.firmware?.[text(client,'companionId')];
  const outdated=(client:Record<string,unknown>)=>deviceKind(client)==='esp32' && !!firmwareInfo(client) && firmwareInfo(client)?.version!==hostState.firmwareVersion;
  async function prepareUpdate(){
    await send('companion_usb_ports',{companionId:selectedId});
    updateProfile=firmwareInfo(selected!)?.profile ?? '';
    const result=await send('companion_usb_update_prepare',{companionId:selectedId});
    updatePort=result.usbPorts?.length===1?result.usbPorts[0].port:'';
    updateOpen=true;
  }
  async function installUpdate(){
    await send('companion_usb_flash',{companionId:selectedId,port:updatePort,profile:updateProfile});
    updating=true;
  }

  const clients = $derived(hostState.host?.clients ?? []);
  const text = (row: Record<string, unknown>, key: string) => typeof row[key] === 'string' ? String(row[key]) : '';
  const selected=$derived(clients.find(c=>text(c,'companionId')===selectedId));
  $effect(() => { onViewChange?.(!!pendingCompanionId || !!selected || pairingOpen); });
  const selectedUsb=$derived(hostState.usb?.devices?.find(usb=>usb.device?.companionId===selectedId)??(hostState.usb?.device?.companionId===selectedId?hostState.usb:undefined));
  const detailPort=$derived(updateOpen?updatePort:selectedUsb?.connected?selectedUsb.port:undefined);
  const detailProfile=$derived(updateOpen?updateProfile:selectedUsb?.device?.boardProfile??(selected?firmwareInfo(selected)?.profile:undefined));
  const boardNames:Record<string,string>={'waveshare-lcd7-s3':'Waveshare ESP32-S3 Touch LCD 7 · Rev 1.2 · 800×480','sunton-3248s035r':'Sunton/GUITION ESP32-3248S035R · resistive · 480×320'};
  const detailBoard=$derived(hostState.firmwareProfiles?.find(p=>p.id===detailProfile)?.name??(detailProfile?boardNames[detailProfile]??detailProfile:'Not identified'));

  const deviceKind = (row: Record<string, unknown>) => text(row,'kind') || 'desktop';
  const icon = (type: string) => type === 'esp32' ? 'usb' : type === 'mobile' ? 'mobile' : 'monitor';
  async function send(command: CompanionCommand, companion: Record<string, unknown> = {}) {
    const result = (await gatewayRequest({ command, companion })).companions;
    if (!result) throw new Error('Companion pairing requires the native Cockpit app.');
    hostState = { ...hostState, ...result }; return result;
  }
  async function action(work: () => Promise<void>) {
    if (busy) return; busy=true;
    try { await work(); } catch (error) { notify(String(error),'error'); } finally { busy=false; }
  }
  async function refresh() {
    if (polling || disposed) return; polling=true;
    try {
      await send('companion_status');
      if (disposed) return;
      if (pendingCompanionId && hostState.host?.clients) {
        const client = clients.find(client => text(client, 'companionId') === pendingCompanionId);
        pendingCompanionId = '';
        if (client) edit(client);
        else notify('This companion is no longer paired with Cockpit.', 'error');
      }
      if(updating && hostState.usb?.flashError){updating=false;notify(hostState.usb.flashWarning??hostState.usb.flashError,hostState.usb.flashWarning?'warning':'error');}
      if(updating && hostState.usb?.flashFinished){
        updating=false;await new Promise(resolve=>setTimeout(resolve,1500));
        await send('companion_usb_connect',{port:updatePort});
        updateOpen=false;notify('Firmware updated. Saved pairing and connection settings preserved.');
      }
      if (pairingOpen && newPairingConnected(kind,baseline,hostState)) {
        pairingOpen=false; pairing=undefined; kind=''; notify('Companion paired and saved.');
      }
    } catch (error) { if (!disposed) notify(String(error),'error'); } finally { polling=false; }
  }
  function openPair(type: typeof kind = '') {
    pendingCompanionId = '';
    selectedId='';
    baseline=new Set(clients.map(c=>text(c,'companionId')));
    kind=type; pairing=undefined;browserOfferAttempted=false;pairingOpen=true;
  }
  async function cancel() {
    await send('companion_pair_cancel'); pairingOpen=false; pairing=undefined; kind='';
  }
  async function offer() {
    const result=await send('companion_pair_offer',{kind});
    pairing=result.pairing;
    expires=Date.now()+(pairing?.ttlMs??0); secondsLeft=Math.ceil((pairing?.ttlMs??0)/1000);
  }
  async function remove(client: Record<string,unknown>) {
    if (!window.confirm('Remove '+(text(client,'name') || 'this companion')+'? It must pair again to reconnect.')) return;
    await send('companion_revoke',{companionId:text(client,'companionId')});
    selectedId=''; notify('Companion removed. Pair it again to reconnect.');
  }
  function edit(client:Record<string,unknown>){
    pendingCompanionId = '';
    updateOpen=false;selectedId=text(client,'companionId');editedName=text(client,'name');
    editedPanel=String((hostState.host?.instances??[]).find(i=>i.companionId===selectedId)?.publicationHash??'');
  }
  function backToCompanions() { pendingCompanionId=''; selectedId=''; }
  function focusDetails(node: HTMLButtonElement) { node.focus(); }
  async function saveSettings(){
    const instances=(hostState.host?.instances??[]).filter(i=>i.companionId===selectedId);
    if(instances.length>1)throw new Error('This companion has multiple assignments. Edit assignments in Panels.');
    const panel=publications.find(p=>p.publicationHash===editedPanel);
    if(editedPanel && !panel && editedPanel!==instances[0]?.publicationHash)throw new Error('Choose a saved panel.');
    if(panel)await sendPublication(panel,send);
    await send('companion_update',{companionId:selectedId,name:editedName.trim()});
    if(!editedPanel && instances[0])await send('companion_unassign',{instanceId:instances[0].instanceId});
    else if(panel && instances[0]?.publicationHash!==editedPanel)await send('companion_assign',{companionId:selectedId,instanceId:instances[0]?.instanceId??crypto.randomUUID(),publicationHash:editedPanel});
    await refresh();notify('Companion settings saved.');
  }
  onMount(() => {
    void refresh();
    const timer=setInterval(() => {
      secondsLeft=Math.max(0,Math.ceil((expires-Date.now())/1000));
      if (!secondsLeft && pairing){pairing=undefined;if(kind==='browser')browserOfferAttempted=false;}
      void refresh();
    },1000);
    return () => {disposed=true;clearInterval(timer);if(updateOpen && !updating && updatePort)void send('companion_usb_connect',{port:updatePort}).catch(error=>notify(String(error),'error'));};
  });
</script>

<section class="companions" aria-label="Companions">
  {#if !pendingCompanionId && !selected}
  <header class="page-heading"><div><div class="eyebrow">YOUR PAIRED SCREENS</div><h1>Companions</h1><p>Your paired screens and their connection state.</p></div>
    {#if !pairingOpen}<button disabled={updateOpen || updating} onclick={() => openPair()}><Icon name="plus" size={16} />Pair</button>{/if}
  </header>
  {/if}
  {#if pairingOpen}
    <section class="pair-form" aria-label="Pair companion">
      <div class="form-heading"><h3>Pair a companion</h3><button class="danger" disabled={busy} onclick={() => action(cancel)}><Icon name="close" size={16} />Cancel</button></div>
      <fieldset disabled={busy || !!pairing}><legend>Companion type</legend><div class="types">
        {#each [{id:'browser',name:'Browser',description:'Any browser on your network'}, {id:'mobile',name:'Mobile',description:'Scan a QR code or enter a code'}, {id:'esp32',name:'ESP32',description:'Connect by USB to install and pair'}] as type}
          <label class:chosen={kind===type.id}><input type="radio" name="companion-type" value={type.id} checked={kind===type.id} onchange={() => {kind=type.id as typeof kind;browserOfferAttempted=false;}} /><Icon name={icon(type.id)} size={24} /><strong>{type.name}</strong><span>{type.description}</span></label>
        {/each}
      </div></fieldset>
      {#if kind === 'esp32'}
        <Esp32Companions {publications} />
      {:else if kind}
        {#if !pairing}
          <p>{kind==='browser'?(busy?'Creating pairing link…':'Open the pairing link in any browser on your network. That browser pairs automatically and remembers this Cockpit host.'):'Open the mobile companion and scan the QR code generated here. You can also use the eight-character code.'}</p>
          <div class="pair-actions">
            {#if kind==='mobile'}<button disabled={busy || !hostState.listener?.enabled} onclick={() => action(offer)}><Icon name="link" size={16} />Generate pairing QR code</button>{/if}
            {#if !hostState.listener?.enabled}
              <div class="listener-actions">
                <div class="listener-warning" role="status"><Icon name="alert" size={16} /><span>Enable the companion listener in Settings before pairing over the network.</span></div>
                <button onclick={openSettings}><Icon name="settings" size={16} />Go to settings</button>
              </div>
            {/if}
          </div>
        {:else}
          <div class="pair-offer">
            {#if kind==='browser'}
              <div class="browser-link"><p>Open this one-use link in the browser you want to pair. Expires in {secondsLeft}s.</p><input aria-label="Browser pairing link" readonly value={pairing.browserUrl} /><div class="pair-actions"><button onclick={()=>action(async()=>{await navigator.clipboard.writeText(pairing!.browserUrl!);notify('Pairing link copied.');})}><Icon name="copy" size={16} />Copy link</button><button onclick={()=>action(async()=>{if(pairing?.browserUrl)await invoke('open_companion_browser',{url:pairing.browserUrl});})}><Icon name="link" size={16} />Open in browser</button></div><p>The receiving browser must trust Cockpit’s HTTPS certificate. Export it from Settings → Companion listener and verify its fingerprint before trusting it. After pairing, install the browser app for quick access.</p></div>
            {:else}
            {#if kind==='mobile'}<svg aria-label="Companion pairing QR code" role="img" viewBox={'-4 -4 '+(pairing.qrMatrix.length+8)+' '+(pairing.qrMatrix.length+8)}><rect x="-4" y="-4" width={pairing.qrMatrix.length+8} height={pairing.qrMatrix.length+8} fill="white" />{#each pairing.qrMatrix as row,y}{#each row as dark,x}{#if dark==='1'}<rect {x} {y} width="1" height="1" fill="black" />{/if}{/each}{/each}</svg>{/if}
            <div><p>Enter this code on the mobile companion:</p><output class="pair-code" aria-label="Pairing code">{pairing.code}</output><p>Choose this Cockpit host: <strong>{hostState.listener?.endpoint}</strong></p><p>Expires in {secondsLeft}s. Pairing completes automatically and is saved on both devices.</p></div>
            {/if}
          </div>
        {/if}
      {/if}
    </section>
  {:else if pendingCompanionId}
    <section aria-label="Companion settings"><button class="text-button back-companions" use:focusDetails onclick={backToCompanions}><Icon name="back" size={16} />Back to Companions</button><p role="status">Loading companion details…</p></section>
  {:else if selected}
    <section aria-label="Companion settings">
      <button class="text-button back-companions" use:focusDetails disabled={busy || updateOpen || updating} onclick={backToCompanions}><Icon name="back" size={16} />Back to Companions</button>
      <div class="page-heading"><div><div class="eyebrow">COMPANION SETTINGS</div><h1>{text(selected,'name') || 'Companion'}</h1><p>Connection and panel assignment.</p></div><span class:connected={selected.connected===true} class="connection-state"><span></span>{selected.connected===true?'Connected':'Offline'}</span></div>
      <div class="settings-panel">
      {#if hostState.usb?.device?.companionId===selectedId && hostState.usb.device.status==='error' && hostState.usb.deviceStatus==='panel-unsupported-corrupt-or-over-budget'}
        <Notification message="The assigned panel contains unsupported content or exceeds this screen’s limits. Choose a compatible panel to reconnect." kind="error" />
      {/if}
      {#if ['browser','desktop'].includes(deviceKind(selected)) && hostState.browserUrl}<button onclick={()=>action(async()=>{await invoke('open_companion_browser',{url:hostState.browserUrl});})}><Icon name="link" size={16} />Open browser companion</button>{/if}
      <div class="settings-columns" class:with-firmware={deviceKind(selected)==='esp32'}>
      <div class="identity-settings">
      <label class="setting">Name<input bind:value={editedName} maxlength="80" /></label>
      <label class="setting">Assigned panel<select bind:value={editedPanel}><option value="">No panel</option>
        {#if editedPanel && !publications.some(p=>p.publicationHash===editedPanel)}<option value={editedPanel}>Current assigned panel</option>{/if}
        {#each publications as panel}<option value={panel.publicationHash}>{panel.document.ui.title}</option>{/each}
      </select></label>
      {#if deviceKind(selected)==='esp32'}
        <label class="setting">USB port<input readonly value={detailPort??'Not connected by USB'} /></label>
        <label class="setting">Board / Screen<input readonly value={detailBoard} /></label>
      {/if}
      </div>
      {#if deviceKind(selected)==='esp32'}
        <section class="firmware-settings" aria-label="Firmware">
          <h3>Firmware</h3>
          <p>{outdated(selected)?'Firmware update available':firmwareInfo(selected)?.version===hostState.firmwareVersion?'Firmware is up to date':'Connect this companion by USB to check its firmware.'}</p>
          <div class="firmware-versions">
            <p>Current: {firmwareInfo(selected)?.version??'Unknown'}</p>
            {#if hostState.firmwareVersion}<p>Available: {hostState.firmwareVersion}</p>{/if}
          </div>
          {#if !updateOpen && (!hostState.firmwareVersion || firmwareInfo(selected)?.version!==hostState.firmwareVersion)}
            <button class="firmware-update" disabled={busy || updating} onclick={()=>action(prepareUpdate)}><Icon name="download" size={16} />Update firmware</button>
          {:else if updateOpen}
            <p>Pairing and Wi-Fi settings are preserved. Keep this companion connected by USB throughout the update.</p>

            <div class="pair-actions"><button disabled={busy || updating || !updatePort || !updateProfile} onclick={()=>action(installUpdate)}><Icon name="download" size={16} />Install update</button>
              <button class="danger" disabled={busy || updating} onclick={()=>action(async()=>{if(updatePort)await send('companion_usb_connect',{port:updatePort});updateOpen=false;})}><Icon name="close" size={16} />Cancel</button></div>
            {#if updating}<p role="status">{hostState.usb?.stage} · {hostState.usb?.progress??0}%</p>{/if}
          {/if}
        </section>
      {/if}
      </div>
      <div class="settings-actions">
        <button disabled={busy || updateOpen || updating || !editedName.trim()} onclick={()=>action(saveSettings)}><Icon name="save" size={16} />Save</button>
        <button class="danger" disabled={busy || updateOpen || updating} onclick={()=>action(()=>remove(selected!))}><Icon name="trash" size={15} />Remove</button>
      </div>
      </div>
    </section>
  {:else if clients.length}
    <div class="companion-tiles">{#each clients as client (text(client,'companionId'))}
      <article class="companion-tile"><button class="tile-open" onclick={()=>edit(client)} aria-label={'Edit '+(text(client,'name') || 'companion')}>
        <div class="tile-heading"><Icon name={icon(deviceKind(client))} size={27} /><span class:connected={client.connected===true} class="connection-state"><span></span>{client.connected===true?'Connected':'Offline'}</span></div>
        <h3>{text(client,'name') || 'Companion'}</h3><p class="kind">{deviceKind(client)==='esp32'?'ESP32 screen':deviceKind(client)==='mobile'?'Mobile companion':['browser','desktop'].includes(deviceKind(client))?'Browser companion':'Companion'}</p>
        <p>{(hostState.host?.instances ?? []).filter(i=>i.companionId===client.companionId).map(i=>text(i,'name') || 'Panel').join(', ') || 'No panel assigned'}</p>
        {#if outdated(client)}<p class="firmware-warning"><Icon name="alert" size={15} />Firmware update available</p>
        {:else if deviceKind(client)==='esp32' && !firmwareInfo(client)}<p>Firmware version unknown · connect USB to check</p>{/if}
        </button>
      </article>
    {/each}</div>
  {:else}<div class="empty"><Icon name="monitor" size={30} /><h3>No companions paired</h3><p>Pair a browser, mobile device or ESP32 screen to get started.</p></div>{/if}
</section>
<style>
  .back-companions{background:transparent;border:0;color:var(--accent);padding:6px 0;margin-bottom:18px;font-size:10px}.back-companions:hover{background:transparent;text-decoration:underline}.settings-panel{padding:21px;border:1px solid var(--line);border-radius:9px;background:var(--surface)}.settings-panel .settings-columns{margin-top:0}.page-heading h1{overflow-wrap:anywhere}.page-heading .connection-state{flex-shrink:0}@media(max-width:650px){.page-heading{align-items:flex-start;flex-wrap:wrap}.settings-panel{padding:16px}}
  .firmware-versions{display:grid;gap:6px;margin-top:12px}.firmware-versions p{margin:0}
  .settings-columns{display:grid;gap:28px;margin-top:18px}.settings-columns.with-firmware{grid-template-columns:minmax(0,1fr) minmax(0,1fr)}.identity-settings{min-width:0}.identity-settings .setting:first-child{margin-top:0}.firmware-settings{min-width:0;border-left:1px solid var(--line,#344452);padding-left:28px}.firmware-settings h3{margin-top:0}@media(max-width:800px){.settings-columns.with-firmware{grid-template-columns:1fr}.firmware-settings{border-left:0;border-top:1px solid var(--line,#344452);padding-left:0;padding-top:20px}}.firmware-update{margin-top:16px}.firmware-warning{display:flex;align-items:center;gap:7px;color:var(--warning,#e9b75c)}
  .browser-link{width:100%;display:grid;gap:18px}.browser-link p{margin:0}.browser-link input{width:100%;font:inherit;padding:10px;background:var(--surface-raised);color:var(--text);border:1px solid var(--line,#344452);border-radius:6px}.browser-link .pair-actions{justify-content:flex-start;margin:0;gap:12px}
  .tile-open{display:block;width:100%;padding:0;border:0;background:transparent;text-align:left}.tile-open:hover{background:transparent}.tile-open:focus-visible{outline:2px solid var(--accent,#64caba);outline-offset:8px}.setting{display:grid;gap:7px;font-size:13px;margin:18px 0;max-width:480px}.setting input,.setting select{font:inherit;padding:10px;border:1px solid var(--line,#344452);border-radius:6px;color:var(--text);background:var(--surface-raised)}.settings-actions{display:flex;justify-content:flex-end;flex-wrap:wrap;gap:10px;margin-top:28px}.danger{background:#9b2430;border-color:#bc3441;color:#fff}.danger:hover{background:#b32b38;border-color:#e35d69}
  header,.form-heading,.tile-heading{display:flex;justify-content:space-between;align-items:center;gap:16px}h3{font-size:16px;margin:15px 0 8px}p{font-size:13px;color:var(--muted,#9aafbf);line-height:1.6}button{display:inline-flex;align-items:center;justify-content:center;gap:7px;font:inherit;font-size:12px;padding:9px 12px;border:1px solid var(--line,#344452);border-radius:6px;background:var(--surface-raised,#14212c);color:var(--text,#e1eaf0);cursor:pointer}button:hover{border-color:var(--accent,#64caba)}button:disabled{opacity:.45;cursor:default}.companion-tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:18px;margin-top:22px}.companion-tile,.pair-form,.empty{border:1px solid var(--line,#344452);border-radius:9px;padding:20px;background:var(--surface,#14212c)}.companion-tile{min-width:0;position:relative}.tile-open::after{content:'';position:absolute;inset:0;border-radius:9px}.tile-heading{color:var(--accent,#64caba)}.connection-state{display:flex;align-items:center;gap:6px;color:var(--muted,#9aafbf);font-size:12px}.connection-state>span{width:7px;height:7px;border-radius:50%;background:currentColor}.connection-state.connected{color:var(--accent,#64caba)}.kind{text-transform:capitalize}.pair-form{margin-top:20px}.form-heading h3{margin:0}.types{display:grid;grid-template-columns:repeat(3,1fr);gap:12px}.types label{display:flex;flex-direction:column;gap:9px;border:1px solid var(--line,#344452);border-radius:8px;padding:18px;cursor:pointer;position:relative}.types input{position:absolute;right:12px;top:12px}.types span{font-size:12px;color:var(--muted,#9aafbf)}.types .chosen{border-color:var(--accent,#64caba);background:var(--accent-bg,#18372e)}fieldset{border:0;padding:0;margin:22px 0}legend{font-size:13px;margin-bottom:12px}.pair-offer{display:flex;gap:25px;align-items:center;flex-wrap:wrap;margin:20px 0}.pair-offer svg{width:240px;shape-rendering:crispEdges;flex-shrink:0}.pair-code{display:block;font:700 34px monospace;letter-spacing:6px;color:var(--text,#e1eaf0);padding:15px 0}.empty{text-align:center;margin-top:22px;padding:35px;color:var(--muted,#9aafbf)}.pair-actions{display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap;margin-top:24px}.pair-actions>button,.listener-actions>button{flex-shrink:0}.listener-actions{display:flex;align-items:center;justify-content:flex-end;gap:12px;margin-left:auto;flex-wrap:wrap}.listener-warning{display:flex;align-items:center;gap:8px;color:var(--warning,#e9b75c);font-size:13px;line-height:1.5}.listener-warning :global(svg){flex-shrink:0}@media(max-width:650px){.types{grid-template-columns:1fr}.pair-code{font-size:25px;letter-spacing:3px}}
</style>
