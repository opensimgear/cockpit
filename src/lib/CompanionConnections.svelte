<script lang="ts">
  import PageHeader from './PageHeader.svelte';
  import { onMount, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import CompanionMessage from './CompanionMessage.svelte';
  import { firmwareUpdateAvailable, compareFirmwareVersions, companionFirmwareVersion } from './companion-firmware';
  import { companionMessages } from './companion-messages';
  import { newPairingConnected } from './companion-pairing';
  import { invoke } from '@tauri-apps/api/core';
  import { gatewayRequest } from './gateway';
  import { notify } from './notifications';
  import { type CompanionCommand, type CompanionHostState } from './companions';
  import { createCompanionSettingsAutosave } from './companion-settings';
  import type { CompanionPublication } from './companions';
  import { companionScreenSize, companionPanelReason, companionRenderTarget } from './companion-screen';
  let { publications = [], openSettings, initialCompanionId = '', onViewChange, onNavigate, active = true }: { publications?: CompanionPublication[]; openSettings: () => void; initialCompanionId?: string; onViewChange?: (detailsOpen: boolean) => void; onNavigate?: (companionId: string | null) => void; active?: boolean } = $props();
  let hostState = $state.raw<CompanionHostState>({});
  let pendingCompanionId = $state(untrack(() => initialCompanionId));
  let disposed = false;
  let mounted = $state(false);
  let pairingOpen = $state(false);
  let browserOfferAttempted=$state(false);
  $effect(()=>{
    if(active && pairingOpen && kind==='browser' && hostState.listener?.enabled && !pairing && !busy && !browserOfferAttempted){
      browserOfferAttempted=true;void action(offer);
    }
  });
  let kind = $state<'' | 'browser' | 'mobile' | 'esp32'>('');
  let busy = $state(false);
  let selectedId=$state(''), editedName=$state(''), editedPanel=$state('');
  let settingsStates = $state.raw<Record<string, { saving: boolean; pending: boolean; error: string }>>({});
  const settingsDrafts = new Map<string, { name: string; panelHash: string; saveName: boolean; savePanel: boolean }>();
  const settingsState = $derived(settingsStates[selectedId]);
  const settingsSaving = $derived(Object.values(settingsStates).some(state => state.saving));
  const nameError = $derived(selectedId && !editedName.trim() ? 'Enter a companion name.' : '');
  const settingsError=$derived(nameError||settingsState?.error||'');
  let pairing = $state<CompanionHostState['pairing']>();
  let secondsLeft = $state(0), expires = 0;
  let baseline = new Set<string>();
  let polling = false;
  let updateOpen=$state(false), updatePort=$state(''), updateProfile=$state(''), updating=$state(false);
  const firmwareInfo=(client:Record<string,unknown>)=>hostState.firmware?.[text(client,'companionId')];
  const outdated=(client:Record<string,unknown>)=>deviceKind(client)==='esp32' && firmwareUpdateAvailable(firmwareInfo(client)?.version,companionFirmwareVersion(hostState,text(client,'companionId')));
  async function prepareUpdate(){
    await send('companion_usb_ports',{companionId:selectedId});
    updateProfile=firmwareInfo(selected!)?.profile ?? '';
    const result=await send('companion_usb_update_prepare',{companionId:selectedId});
    updatePort=result.usbPorts?.length===1?result.usbPorts[0].port:'';
    updateOpen=true;
    await installUpdate();
  }
  async function installUpdate(){
    await send('companion_usb_flash',{companionId:selectedId,port:updatePort,profile:updateProfile});
    updating=true;
  }

  const clients = $derived(hostState.host?.clients ?? []);
  const text = (row: Record<string, unknown>, key: string) => typeof row[key] === 'string' ? String(row[key]) : '';
  const selected=$derived(clients.find(c=>text(c,'companionId')===selectedId));
  const selectedFirmwareVersion=$derived(selected?companionFirmwareVersion(hostState,text(selected,'companionId')):null);
  const firmwareProgress=$derived(Math.max(0,Math.min(100,Math.round(hostState.usb?.progress??0))));
  const selectedMessages=$derived(selected?companionMessages(hostState,selected).filter(message=>message.source==='connection'):[]);
  const selectedScreen=$derived(companionScreenSize(hostState,selectedId));
  const firmwareUpdateDisabledReason = $derived(selected?.connected!==true ? 'This companion is offline. Connect it by USB to update its firmware.' : updating ? 'A firmware update is already in progress.' : busy ? 'Wait for the current operation to finish.' : '');
  const multipleAssignments = $derived((hostState.host?.instances ?? []).filter(instance => instance.companionId===selectedId).length > 1);
  $effect(() => { onViewChange?.(!!pendingCompanionId || !!selected || pairingOpen); });
  const selectedUsb=$derived(hostState.usb?.devices?.find(usb=>usb.device?.companionId===selectedId)??(hostState.usb?.device?.companionId===selectedId?hostState.usb:undefined));
  const detailPort=$derived(updateOpen?updatePort:selectedUsb?.connected?selectedUsb.port:undefined);
  const detailProfile=$derived(updateOpen?updateProfile:selectedUsb?.device?.boardProfile??(selected?firmwareInfo(selected)?.profile:undefined));
  const boardNames:Record<string,string>={'waveshare-lcd7-s3':'Waveshare ESP32-S3 Touch LCD 7 · Rev 1.2 · 800×480','sunton-3248s035r':'Sunton/GUITION ESP32-3248S035R · resistive · 480×320'};
  const detailBoard=$derived(hostState.firmwareProfiles?.find(p=>p.id===detailProfile)?.name??(detailProfile?boardNames[detailProfile]??detailProfile:'Not identified'));

  const deviceKind = (row: Record<string, unknown>) => text(row,'kind') || 'desktop';
  const icon = (type: string) => type === 'esp32' ? 'usb' : type === 'mobile' ? 'mobile' : 'monitor';
  let requestVersion = 0, appliedVersion = 0;
  async function send(command: CompanionCommand, companion: Record<string, unknown> = {}) {
    const version = ++requestVersion;
    const result = (await gatewayRequest({ command, companion })).companions;
    if (!result) throw new Error('Companion pairing requires the native Cockpit app.');
    if (!disposed && version >= appliedVersion) { appliedVersion=version; hostState = { ...hostState, ...result }; }
    return result;
  }
  const settingsAutosave = createCompanionSettingsAutosave(send, state => {
    settingsStates = { ...settingsStates, [state.companionId]: state };
    if (!state.saving && !state.pending && !state.error) settingsDrafts.delete(state.companionId);
    if (state.error && (disposed || selectedId !== state.companionId)) notify(state.error, 'error');
  });
  function scheduleSettings(field: 'name' | 'panel', delayMs = 400) {
    if (!selectedId) return;
    const previous = settingsDrafts.get(selectedId);
    const draft = { name: editedName, panelHash: editedPanel, saveName: field==='name' || !!previous?.saveName, savePanel: field==='panel' || !!previous?.savePanel };
    settingsDrafts.set(selectedId, draft);
    settingsAutosave.schedule({ companionId: selectedId, ...draft, publications }, delayMs);
  }
  async function action(work: () => Promise<void>) {
    if (busy) return; busy=true;
    try { await work(); } catch (error) { notify(String(error),'error'); } finally { busy=false; }
  }
  async function refresh() {
    if (!active || polling || disposed || settingsSaving) return; polling=true;
    try {
      await send('companion_status');
      if (disposed) return;
      if (pendingCompanionId && hostState.host?.clients) {
        const client = clients.find(client => text(client, 'companionId') === pendingCompanionId);
        pendingCompanionId = '';
        if (client) edit(client, false);
        else notify('This companion is no longer paired with Cockpit.', 'error');
      }
      if(updating && hostState.usb?.flashCancelled){
        updating=false;await send('companion_usb_connect',{port:updatePort});updateOpen=false;
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
  $effect(() => { if (active && mounted) untrack(() => { void refresh(); }); });
  function openPair(type: typeof kind = '') {
    void settingsAutosave.flush();
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
    settingsAutosave.discard(text(client,'companionId'));
    settingsDrafts.delete(text(client,'companionId'));
    await send('companion_revoke',{companionId:text(client,'companionId')});
    selectedId=''; onNavigate?.(null); notify('Companion removed. Pair it again to reconnect.');
  }
  function edit(client:Record<string,unknown>, recordNavigation = true){
    void settingsAutosave.flush();
    pendingCompanionId = '';
    updateOpen=false;selectedId=text(client,'companionId');editedName=text(client,'name');
    editedPanel=String((hostState.host?.instances??[]).find(i=>i.companionId===selectedId)?.publicationHash??'');
    const draft=settingsDrafts.get(selectedId);
    if(draft){editedName=draft.name;editedPanel=draft.panelHash;}
    if (recordNavigation) onNavigate?.(selectedId);
  }
  export function getCompanionId(): string { return pendingCompanionId || selectedId; }
  export function showCompanion(companionId: string): void {
    void settingsAutosave.flush();
    pairingOpen=false; pairing=undefined; kind='';
    const client=clients.find(client => text(client, 'companionId')===companionId);
    if (client) edit(client, false);
    else { selectedId=''; pendingCompanionId=companionId; void refresh(); }
  }
  export function requestNavigation(continueNavigation: () => void, cancelNavigation?: () => void): void {
    if (busy || updateOpen || updating) { cancelNavigation?.(); return; }
    void settingsAutosave.flush();
    continueNavigation();
  }
  export function showMain(recordNavigation = true): boolean {
    if (busy || updateOpen || updating) return false;
    void settingsAutosave.flush();
    pendingCompanionId=''; selectedId='';
    if (pairingOpen) void action(cancel);
    if (recordNavigation) onNavigate?.(null);
    return true;
  }
  function backToCompanions() { showMain(); }
  onMount(() => {
    mounted=true;
    const timer=setInterval(() => {
      secondsLeft=Math.max(0,Math.ceil((expires-Date.now())/1000));
      if (!secondsLeft && pairing){pairing=undefined;if(kind==='browser')browserOfferAttempted=false;}
      void refresh();
    },1000);
    return () => {void settingsAutosave.flush();disposed=true;clearInterval(timer);if(updateOpen && !updating && updatePort)void send('companion_usb_connect',{port:updatePort}).catch(error=>notify(String(error),'error'));};
  });
</script>

<section class="companions" aria-label="Companions">
  {#if !pendingCompanionId && !selected}
  <PageHeader title="Companions" eyebrow="YOUR PAIRED SCREENS" subtitle="Your paired screens and their connection state.">{#snippet actions()}
    {#if !pairingOpen}<button disabled={updateOpen || updating} onclick={() => openPair()}><Icon name="plus" size={16} />Pair</button>{/if}
  {/snippet}</PageHeader>
  {/if}
  {#if pairingOpen}
    <section class="pair-form" aria-label="Pair companion">
      <div class="form-heading"><h3>Pair a companion</h3><button class="danger" disabled={busy} onclick={() => action(cancel)}><Icon name="close" size={16} />Cancel</button></div>
      <fieldset disabled={busy || !!pairing}><legend>Companion type</legend><div class="types">
        {#each [{id:'browser',name:'Browser',description:'Any browser on your network'}, {id:'mobile',name:'Mobile',description:'Scan a QR code or enter a code'}, {id:'esp32',name:'ESP32',description:'Pairing is temporarily unavailable.'}] as type}
          <label class:chosen={kind===type.id} class:unavailable={type.id==='esp32'}><input type="radio" name="companion-type" value={type.id} checked={kind===type.id} disabled={type.id==='esp32'} onchange={() => {kind=type.id as typeof kind;browserOfferAttempted=false;}} /><Icon name={icon(type.id)} size={24} /><strong>{type.name}</strong><span>{type.description}</span></label>
        {/each}
      </div></fieldset>
      {#if kind === 'esp32'}
        <p>ESP32 pairing is temporarily unavailable.</p>
      {:else if kind}
        {#if !pairing}
          <p>{kind==='browser'?(busy?'Creating pairing link…':'Open the pairing link in any browser on your network. That browser pairs automatically and remembers this Cockpit host.'):'Open the mobile companion and scan the QR code generated here. You can also use the eight-character code.'}</p>
          <div class="pair-actions">
            {#if kind==='mobile'}<button disabled={busy || !hostState.listener?.enabled} onclick={() => action(offer)}><Icon name="link" size={16} />Generate pairing QR code</button>{/if}
            {#if !hostState.listener?.enabled}
              <div class="listener-actions">
                <CompanionMessage kind="warning" message="Enable the companion listener in Settings before pairing over the network."/>
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
    <section aria-label="Companion settings"><PageHeader title="Companion" subtitle="Settings and connection details." onback={backToCompanions} backLabel="Back to Companions" focusBack/><p role="status">Loading companion details…</p></section>
  {:else if selected}
    <section aria-label="Companion settings">
      <PageHeader title={text(selected,'name') || 'Companion'} eyebrow="COMPANION SETTINGS" subtitle="Settings and connection details." onback={backToCompanions} backLabel="Back to Companions" backDisabled={busy || updateOpen || updating} focusBack>
        {#snippet actions()}<span class:connected={selected.connected===true} class="connection-state"><span></span>{selected.connected===true?'Connected':'Offline'}</span><div class="remove-companion">
          <button class="danger" aria-describedby={updateOpen || updating ? 'companion-remove-reason' : undefined} disabled={busy || updateOpen || updating || settingsState?.saving || settingsState?.pending && !settingsState.error} onclick={()=>action(()=>remove(selected!))}><Icon name="trash" size={15} />Remove</button>
          {#if updateOpen || updating}<p id="companion-remove-reason" role="status">{updating && !hostState.usb?.waitingForBoot ? 'Wait for the firmware update to finish before removing this companion.' : 'Finish or cancel the firmware update before removing this companion.'}</p>{/if}
        </div>
      {/snippet}
      </PageHeader>
      {#if settingsError || selectedMessages.length}
        <div class="companion-status-messages">
          {#if settingsError}<div id="companion-autosave-status" class="companion-notice error" role="alert"><strong>Error</strong><span>{settingsError}</span></div>{/if}
          {#each selectedMessages as message (`${message.source}:${message.message}`)}
            <div class="companion-notice" class:error={message.kind==='error'} class:warning={message.kind==='warning'} role={message.kind==='error'?'alert':'status'}>
              {#if message.kind!=='info'}<strong>{message.kind==='error'?'Error':'Warning'}</strong>{/if}<span>{message.message}</span>
            </div>
          {/each}
        </div>
      {/if}
      <div class="settings-panel">
      {#if ['browser','desktop'].includes(deviceKind(selected)) && hostState.browserUrl}<button onclick={()=>action(async()=>{await invoke('open_companion_browser',{url:hostState.browserUrl});})}><Icon name="link" size={16} />Open browser companion</button>{/if}
      <div class="settings-columns" class:with-firmware={deviceKind(selected)==='esp32'}>
      <div class="identity-settings">
      <label class="setting">Name<input value={editedName} oninput={event => {editedName=event.currentTarget.value;scheduleSettings('name');}} onblur={() => void settingsAutosave.flush()} disabled={busy || updateOpen || updating} maxlength="80" aria-invalid={!!nameError} aria-describedby={settingsError?'companion-autosave-status':undefined} /></label>
      <label class="setting">Assigned panel<select value={editedPanel} onchange={event => {editedPanel=event.currentTarget.value;scheduleSettings('panel',0);}} disabled={busy || updateOpen || updating || multipleAssignments}><option value="">No panel</option>
        {#if editedPanel && !publications.some(p=>p.publicationHash===editedPanel)}<option value={editedPanel}>Current assigned panel</option>{/if}
        {#each publications as panel}<option value={panel.publicationHash} title={companionPanelReason(panel,hostState,selectedId)||undefined} disabled={!!companionPanelReason(panel,hostState,selectedId)}>{panel.document.ui.title}{companionPanelReason(panel,hostState,selectedId)?' · Not compatible':''}</option>{/each}
      </select></label>
      {#if multipleAssignments}<CompanionMessage kind="warning" message="This companion has multiple assignments. Edit panel assignments in Panels."/>{/if}
      {#if deviceKind(selected)==='esp32'}
        <label class="setting">USB port<input readonly value={detailPort??'Not connected by USB'} /></label>
        <label class="setting">Board / Screen<input readonly value={detailBoard} /></label>
        <div class="setting"><label for="companion-screen-size">Screen size</label><div class="screen-size-input">
          <input id="companion-screen-size" readonly value={selectedScreen?`${selectedScreen.width} × ${selectedScreen.height} px`:'Not identified — connect by USB'} />
          <span class="screen-size-info"><CompanionMessage message={selectedScreen?(companionRenderTarget(hostState,selectedId).profile?`Panels fit within this ${selectedScreen.width} × ${selectedScreen.height} px screen. Interactive hit areas must stay at least 44 × 44 px.`:`Panels must use ${selectedScreen.width} × ${selectedScreen.height} px on every page.`):'Connect this companion by USB to identify its screen size before assigning a panel.'}/></span>
        </div></div>
      {/if}
      </div>
      {#if deviceKind(selected)==='esp32'}
        <section class="firmware-settings" aria-label="Firmware">
          <div class="firmware-heading"><h3>Firmware</h3><CompanionMessage kind={outdated(selected)?'warning':'info'} message={outdated(selected)?'Firmware update available':selectedFirmwareVersion && firmwareInfo(selected)?.version && (compareFirmwareVersions(firmwareInfo(selected)!.version,selectedFirmwareVersion!)??-1)>0?'Installed firmware is newer than the available release':selectedFirmwareVersion && firmwareInfo(selected)?.version===selectedFirmwareVersion?'Firmware is up to date':hostState.firmwareError||'Check for available firmware, then connect this companion by USB to update.'}/></div>
          <div class="firmware-copy">
            <dl class="firmware-versions">
              <div><dt>Current</dt><dd>{firmwareInfo(selected)?.version??'Unknown'}</dd></div>
              <div><dt>Available</dt><dd class="firmware-version-value"><span>{selectedFirmwareVersion??'Unknown'}</span><button class="firmware-refresh" aria-label="Refresh available firmware" title="Refresh available firmware" disabled={busy || updating || updateOpen} onclick={()=>action(async()=>{await send('companion_firmware_check');})}><Icon name="refresh" size={16} /></button></dd></div>
            </dl>
            <p>{hostState.firmwareSource==='development'?'Development firmware from this Cockpit build':'Firmware from published ESP32 companion releases'}</p>
          </div>
          {#if !updateOpen && outdated(selected)}
            <div class="firmware-actions">
              <span class="firmware-update" title={firmwareUpdateDisabledReason || undefined} aria-label={firmwareUpdateDisabledReason || undefined}><button disabled={!!firmwareUpdateDisabledReason} onclick={()=>action(prepareUpdate)}><Icon name="download" size={16} />Update firmware</button></span>
            </div>
          {:else if updateOpen}
            <div class="firmware-update-details">
              <p class="boot-instructions">Switch the device to boot mode: hold BOOT, tap RESET while holding BOOT, then release BOOT. Keep the USB cable connected. The upgrade starts automatically. Pairing and Wi-Fi settings are preserved.</p>
              <div class="firmware-progress-row">
                <div class="firmware-progress" role="progressbar" aria-label="Firmware update progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={hostState.usb?.waitingForBoot ? undefined : firmwareProgress} aria-valuetext={hostState.usb?.stage}>
                  <div class="firmware-progress-fill" style:width={(hostState.usb?.waitingForBoot?0:firmwareProgress)+'%'}></div>
                  <span>{hostState.usb?.stage || 'Preparing firmware update...'}{hostState.usb?.waitingForBoot ? '' : ` · ${firmwareProgress}%`}</span>
                </div>
                <button class="danger" disabled={busy || (updating && (!hostState.usb?.waitingForBoot || hostState.usb?.cancelRequested))} onclick={()=>action(async()=>{
                  if(updating) await send('companion_usb_flash_cancel');
                  else {if(updatePort)await send('companion_usb_connect',{port:updatePort});updateOpen=false;}
                })}>Cancel</button>
              </div>
            </div>
          {/if}
        </section>
      {/if}
      </div>
      {#if settingsState?.error && !nameError}<button aria-describedby="companion-autosave-status" disabled={settingsState.saving} onclick={() => void settingsAutosave.flush()}>Retry</button>{/if}
      </div>
    </section>
  {:else if clients.length}
    <div class="companion-tiles">{#each clients as client (text(client,'companionId'))}
      {@const messages=companionMessages(hostState,client)}
      <article class="companion-tile"><button class="tile-open" onclick={()=>edit(client)} aria-label={'Edit '+(text(client,'name') || 'companion')}>
        <div class="tile-heading"><Icon name={icon(deviceKind(client))} size={27} /><span class:connected={client.connected===true} class="connection-state"><span></span>{client.connected===true?'Connected':'Offline'}</span></div>
        <h3>{text(client,'name') || 'Companion'}</h3><p class="kind">{deviceKind(client)==='esp32'?'ESP32 screen':deviceKind(client)==='mobile'?'Mobile companion':['browser','desktop'].includes(deviceKind(client))?'Browser companion':'Companion'}</p>
        <p>{(hostState.host?.instances ?? []).filter(i=>i.companionId===client.companionId).map(i=>text(i,'name') || 'Panel').join(', ') || 'No panel assigned'}</p>
        </button>
        {#if messages.length}<div class="tile-messages">{#each messages as message (`${message.source}:${message.message}`)}<CompanionMessage message={message.message} kind={message.kind}/>{/each}</div>{/if}
      </article>
    {/each}</div>
  {:else}<div class="empty"><Icon name="monitor" size={30} /><h3>No companions paired</h3><p>Pair a browser or mobile device to get started.</p></div>{/if}
</section>
<style>
  .types label.unavailable{opacity:.55;cursor:not-allowed}
  .firmware-version-value{display:flex;align-items:center;gap:8px}.firmware-version-value>span{min-width:0}.firmware-refresh{display:inline-flex;align-items:center;justify-content:center;width:28px;height:28px;padding:4px;flex-shrink:0}
  .tile-messages,.firmware-heading{display:flex;align-items:center;gap:8px;flex-wrap:wrap}.tile-messages{position:absolute;right:12px;bottom:12px;flex-wrap:nowrap}.companion-status-messages{display:grid;gap:8px;margin:0 0 18px;min-width:0}.companion-notice{display:grid;gap:4px;padding:12px 14px;border:1px solid var(--line,#344452);border-left:3px solid var(--accent,#64caba);border-radius:6px;background:var(--surface,#14212c);font-size:13px;line-height:1.5;overflow-wrap:anywhere}.companion-notice strong{font-size:12px}.companion-notice.error{border-color:#ff899366;background:color-mix(in srgb,#ff8993 8%,var(--surface,#14212c))}.companion-notice.error strong{color:#ff8993}.companion-notice.warning{border-color:#efc26966;background:color-mix(in srgb,#efc269 8%,var(--surface,#14212c))}.companion-notice.warning strong{color:#efc269}.firmware-heading h3{margin:0}
  .setting input[readonly],.setting input:disabled,.setting select:disabled{color:var(--muted);background:var(--input-bg);border-color:var(--faint-line);cursor:default}.setting:has(input[readonly]),.setting:has(input:disabled),.setting:has(select:disabled){color:var(--muted)}
  .screen-size-input{position:relative;min-width:0}.setting .screen-size-input input{box-sizing:border-box;width:100%;padding-right:42px}.screen-size-info{position:absolute;right:8px;top:0;bottom:0;display:flex;align-items:center}
  .remove-companion{display:grid;justify-items:end;gap:8px;max-width:250px}.remove-companion p{margin:0;font-size:12px;text-align:right}
  .connection-state{display:inline-flex;align-items:center;gap:8px;white-space:nowrap;font-size:13px}.connection-state>span{width:10px;height:10px;flex:0 0 10px}.firmware-update{display:inline-flex}.firmware-update:has(button:disabled){cursor:not-allowed}.firmware-update button:disabled{pointer-events:none}
  .settings-panel{padding:21px;border:1px solid var(--line);border-radius:9px;background:var(--surface)}.settings-panel .settings-columns{margin-top:0}@media(max-width:650px){.settings-panel{padding:16px}}
  .firmware-settings{display:flex;flex-direction:column;align-items:flex-start;gap:18px}.firmware-copy{display:grid;gap:12px;max-width:100%;overflow-wrap:anywhere}.firmware-copy p{margin:0;max-width:42ch}.firmware-versions{display:grid;gap:8px;margin:0;font-size:13px;line-height:1.6}.firmware-versions>div{display:grid;grid-template-columns:72px minmax(0,1fr);gap:14px;align-items:baseline}.firmware-versions dt{color:var(--muted)}.firmware-versions dd{margin:0;color:var(--text);font-variant-numeric:tabular-nums;overflow-wrap:anywhere}.firmware-actions{display:flex;align-items:center;flex-wrap:wrap;gap:10px;max-width:100%}.firmware-actions button,.firmware-update{max-width:100%}.boot-instructions{margin:0}.firmware-progress-row{display:flex;align-items:stretch;gap:16px;width:100%}.firmware-progress-row>button{min-height:36px}.firmware-progress{position:relative;flex:1;min-width:0;min-height:36px;box-sizing:border-box;overflow:hidden;border:1px solid var(--line,#456170);border-radius:5px;background:var(--input-bg,#101c27)}.firmware-progress-fill{position:absolute;inset:0 auto 0 0;background:#326c58;transition:width .2s}.firmware-progress>span{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;padding:4px 12px;box-sizing:border-box;text-align:center;font-weight:600;color:var(--text);font-variant-numeric:tabular-nums}.firmware-update-details{display:grid;justify-items:start;gap:14px;width:100%;min-width:0;padding-top:18px;border-top:1px solid var(--line,#344452)}
  .settings-columns{display:grid;gap:28px;margin-top:18px}.settings-columns.with-firmware{grid-template-columns:minmax(0,1fr) minmax(0,1fr)}.identity-settings{min-width:0}.identity-settings .setting:first-child{margin-top:0}.firmware-settings{min-width:0;border-left:1px solid var(--line,#344452);padding-left:28px}.firmware-settings h3{margin-top:0}@media(max-width:800px){.settings-columns.with-firmware{grid-template-columns:1fr}.firmware-settings{border-left:0;border-top:1px solid var(--line,#344452);padding-left:0;padding-top:20px}}
  .browser-link{width:100%;display:grid;gap:18px}.browser-link p{margin:0}.browser-link input{width:100%;font:inherit;padding:10px;background:var(--surface-raised);color:var(--text);border:1px solid var(--line,#344452);border-radius:6px}.browser-link .pair-actions{justify-content:flex-start;margin:0;gap:12px}
  .tile-open{display:block;width:100%;padding:0 0 16px;border:0;background:transparent;text-align:left}.tile-open:hover{background:transparent}.tile-open:focus-visible{outline:2px solid var(--accent,#64caba);outline-offset:8px}.setting{display:grid;gap:7px;font-size:13px;margin:18px 0;max-width:480px}.setting input,.setting select{min-width:0;box-sizing:border-box;width:100%;font:inherit;padding:10px;border:1px solid var(--line,#344452);border-radius:6px;color:var(--text);background:var(--surface-raised)}.danger{background:#9b2430;border-color:#bc3441;color:#fff}.danger:hover{background:#b32b38;border-color:#e35d69}
  .form-heading,.tile-heading{display:flex;justify-content:space-between;align-items:center;gap:16px}h3{font-size:16px;margin:15px 0 8px}p{font-size:13px;color:var(--muted,#9aafbf);line-height:1.6}button{display:inline-flex;align-items:center;justify-content:center;gap:7px;font:inherit;font-size:12px;padding:9px 12px;border:1px solid var(--line,#344452);border-radius:6px;background:var(--surface-raised,#14212c);color:var(--text,#e1eaf0);cursor:pointer}button:hover{border-color:var(--accent,#64caba)}button:disabled{opacity:.45;cursor:default}.companion-tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:18px;margin-top:22px}.companion-tile,.pair-form,.empty{border:1px solid var(--line,#344452);border-radius:9px;padding:20px;background:var(--surface,#14212c)}.companion-tile{min-width:0;position:relative}.tile-open::after{content:'';position:absolute;inset:0;border-radius:9px}.tile-heading{color:var(--accent,#64caba)}.connection-state{display:flex;align-items:center;gap:6px;color:var(--muted,#9aafbf);font-size:12px}.connection-state>span{width:7px;height:7px;border-radius:50%;background:currentColor}.connection-state.connected{color:var(--accent,#64caba)}.kind{text-transform:capitalize}.pair-form{margin-top:20px}.form-heading h3{margin:0}.types{display:grid;grid-template-columns:repeat(3,1fr);gap:12px}.types label{display:flex;flex-direction:column;gap:9px;border:1px solid var(--line,#344452);border-radius:8px;padding:18px;cursor:pointer;position:relative}.types input{position:absolute;right:12px;top:12px}.types span{font-size:12px;color:var(--muted,#9aafbf)}.types .chosen{border-color:var(--accent,#64caba);background:var(--accent-bg,#18372e)}fieldset{border:0;padding:0;margin:22px 0}legend{font-size:13px;margin-bottom:12px}.pair-offer{display:flex;gap:25px;align-items:center;flex-wrap:wrap;margin:20px 0}.pair-offer svg{width:240px;shape-rendering:crispEdges;flex-shrink:0}.pair-code{display:block;font:700 34px monospace;letter-spacing:6px;color:var(--text,#e1eaf0);padding:15px 0}.empty{text-align:center;margin-top:22px;padding:35px;color:var(--muted,#9aafbf)}.pair-actions{display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap;margin-top:24px}.pair-actions>button,.listener-actions>button{flex-shrink:0}.listener-actions{display:flex;align-items:center;justify-content:flex-end;gap:12px;margin-left:auto;flex-wrap:wrap}@media(max-width:650px){.types{grid-template-columns:1fr}.pair-code{font-size:25px;letter-spacing:3px}}
</style>
