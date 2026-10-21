<script lang="ts">
  import { boardScreenSize, panelScreenReason } from './companion-screen';
  import { notify } from './notifications';
  import Notification from './Notification.svelte';
  import Icon from './Icon.svelte';
  import CompanionMessage from './CompanionMessage.svelte';
  import { selectedUsbIsPaired } from './companion-pairing';
  import { onMount } from 'svelte';
  import { firmwareMeetsAvailable } from './companion-firmware';
  import { gatewayRequest } from './gateway';
  import { sendPublication, type CompanionCommand, type CompanionHostState } from './companions';
  import type { Publication } from '@opensimgear/companion-designer';
  let { publications = [], companionId = "", initialName = "ESP32 screen" }: { publications?: Publication[]; companionId?: string; initialName?: string } = $props();
  let hostState = $state<CompanionHostState>({});
  let port = $state('');
  let profile = $state('');
  let installing = $state(false);
  let installOpen = $state(false);
  let detecting = $state(false);
  let detectionError=$state('');
  function finishDetection(message=''){
    detecting=false;detectionError=message;
    if(message)notify(message,'warning');
  }
  $effect(()=>{
    if(!detecting || !port)return;
    const selectedPort=port;
    const timeout=setTimeout(()=>{
      if(port===selectedPort && detecting)finishDetection('No firmware response received within 5 seconds. The board might be in bootloader mode. Tap RESET without holding BOOT, then retry detection. Also check the USB connection.');
    },5000);
    return ()=>clearTimeout(timeout);
  });
  let mode = $state<'wifi' | 'serial'>('wifi');
  let name = $state('ESP32 screen');
  let ssid = $state('');
  let password = $state('');
  let passwordChanged=$state(false), savedPassword=$state(false), savedSsid=$state('');
  let prefilledPort='';
  let selectedHash=$state('');
  let step = $state(0);
  let busy = $state(false);
  let polling = false;
  let error = $state('');
  const transport=$derived(hostState.usb);
  const pairedHere=$derived(!!transport?.device?.companionId && (hostState.host?.clients??[]).some(c=>c.companionId===transport?.device?.companionId));
  const usb=$derived(companionId ? (transport?.device?.companionId && transport.device.companionId!==companionId ? undefined : transport) : (pairedHere && step!==4 ? undefined : transport));
  const device=$derived(usb?.device);
  const screenSize=$derived(boardScreenSize(device?.boardProfile));
  const pairingPorts=$derived((hostState.usbPorts??[]).filter(item=>!!companionId || !(pairedHere && transport?.port===item.port)));
  const selectedDevice=$derived(!!port && usb?.connected && usb.ready && (companionId ? device?.companionId===companionId : !pairedHere) && usb.port===port);
  const identifiedBoard=$derived(selectedDevice && hostState.firmwareProfiles?.some(p=>p.id===device?.boardProfile)?device?.boardProfile:undefined);
  const installedVersion=$derived(selectedDevice?device?.firmwareVersion:undefined);
  const firmwareCurrent=$derived(!detecting && !!hostState.firmwareVersion && firmwareMeetsAvailable(installedVersion,hostState.firmwareVersion) && device?.boardProfile===profile);
  const pairDisabledReason=$derived(!port?'Choose a USB device first.':detecting?'Wait for firmware detection to finish.':installing?'Wait for the firmware installation to finish.':installOpen?'Finish or cancel the firmware installation first.':busy?'Wait for the current operation to finish.':!profile?'Choose the Board / Screen model first.':!selectedDevice?'Install the current firmware and wait for the device to respond.':!firmwareCurrent?'Update the device firmware before pairing.':!name.trim()?'Enter a screen name.':mode==='wifi' && !ssid.trim()?'Choose or enter a Wi-Fi network.':selectedHash?(publications.find(panel=>panel.publicationHash===selectedHash)?panelScreenReason(publications.find(panel=>panel.publicationHash===selectedHash)!,screenSize):'Choose an available panel.'):'');
  const firmwareProgress=$derived(Math.max(0,Math.min(100,Math.round(transport?.progress??0))));
  async function selectPort(value:string){
    if(value!==port)profile='';
    finishDetection();
    port=value;step=0;installOpen=false;detecting=!!value;
prefilledPort='';password='';passwordChanged=false;savedPassword=false;savedSsid='';
    try{
      await send('companion_usb_disconnect');
      if(value)await send('companion_usb_connect',{port:value});
    }catch(error){finishDetection();throw error;}
  }
  async function prepareInstall(){
    if(!hostState.firmwareProfiles?.some(p=>p.id===profile && p.available))throw new Error('Choose the Board / Screen model before installing firmware.');
    await send('companion_usb_ports',{...(companionId?{companionId}:{})});
    await send('companion_usb_disconnect');
    installOpen=true;
    await send('companion_usb_flash',{port,profile});
    installing=true;
  }
  async function send(command: CompanionCommand, companion: Record<string, unknown> = {}) {
    const result = (await gatewayRequest({ command, companion })).companions;
    if (!result) throw new Error('USB setup requires the updated native Cockpit app.');
    hostState = { ...hostState, ...result }; return result;
  }
  async function action(work: () => Promise<void>) {
    if (busy) return; busy = true; error = '';
    try { await work(); } catch (e) { error = String(e); } finally { busy = false; }
  }
  async function refresh() {
    if (busy || polling) return; polling = true;
    try {
      await send('companion_status');
      if(detecting && transport?.port===port){
        if(transport.ready)finishDetection();
        else if(transport.connected===false)finishDetection('The device did not respond. Check its USB connection, then retry detection.');
      }
      if(!companionId && selectedUsbIsPaired(hostState,port) && step!==4 && !installing){port='';finishDetection();}
      if(selectedDevice && prefilledPort!==port){
        prefilledPort=port;
        if(device?.setupSaved){
          mode=device.connectionMode==='serial'?'serial':'wifi';
          if(device.name)name=device.name;
          if(typeof device.ssid==='string')ssid=device.ssid;
          savedSsid=device.ssid??'';savedPassword=!!device.wifiPasswordSaved;passwordChanged=false;
        }
      }
      if(selectedDevice && device?.boardProfile && hostState.firmwareProfiles?.some(p=>p.id===device.boardProfile))profile=device.boardProfile;
      if (installing && transport?.flashError) { installing=false;if(transport.flashWarning)notify(transport.flashWarning,'warning');else throw new Error(transport.flashError); }
      if (installing && transport?.flashFinished) { installing=false; await new Promise(resolve=>setTimeout(resolve,1500)); await selectPort(port); }
    } catch (e) { error = String(e); } finally { polling = false; }
  }
  async function provision() {
    if(!firmwareCurrent)throw new Error('Update the selected device firmware before pairing.');
    try {
      const panel=publications.find(p=>p.publicationHash===selectedHash);
      if(selectedHash && !panel)throw new Error('Choose an available panel.');
      if(panel){const reason=panelScreenReason(panel,screenSize);if(reason)throw new Error(reason);}
      if(panel)await sendPublication(panel,send);
      await send('companion_usb_provision', { mode, name: companionId ? initialName : name, ssid, password, keepWifiPassword:!passwordChanged && savedPassword && ssid===savedSsid, publicationHash: selectedHash, ...(companionId?{companionId}:{}) });
      step = 4;
    } finally { password = ''; }
  }
  onMount(() => {
    name=initialName;
    void send('companion_firmware_check').catch(e => error = String(e));
    void send('companion_usb_ports',{...(companionId?{companionId}:{})}).then(async()=>{await send('companion_status');if(companionId && device?.companionId===companionId){port=usb?.port??'';mode=device?.connectionMode==='serial'?'serial':'wifi';}}).catch(e => error = String(e));
    const timer = setInterval(refresh,1000);
    return () => { clearInterval(timer);password = ''; };
  });
</script>

<section class="usb" aria-label="ESP32 companion setup">
  <header><div><h3>ESP32 companion</h3><p>Connect the screen by USB for setup. Choose Wi-Fi or keep it connected directly.</p></div><CompanionMessage message={port&&usb?.port===port?usb.stage||'USB connected':'Choose a USB device'}/></header>
  {#if companionId && usb?.lastHostError}<Notification message={usb.lastHostError} kind="error" />{/if}
  {#if error}<Notification message={String(error)} kind="error" />{/if}
  {#if usb?.scanError}<Notification message={usb.scanError} kind="error" />{/if}
  <div class="setup-columns">
    <div class="device-controls">
      <div class="row device-row">
        <label>USB device<select value={port} disabled={busy || installing || installOpen} onchange={e=>action(()=>selectPort(e.currentTarget.value))}><option value="">Choose a device</option>{#each pairingPorts as item}<option value={item.port}>{item.name} ({item.port})</option>{/each}</select></label>
        <button disabled={busy || installing || installOpen} onclick={() => action(async () => { await send('companion_usb_ports',{...(companionId?{companionId}:{})}); })}><Icon name="refresh" size={16} />Refresh</button>
      </div>
      {#if !pairingPorts.length}<CompanionMessage message={companionId?'Connect this companion by USB to change its connection settings.':'No unpaired USB devices detected. Connect another screen, or remove an existing companion before pairing it again.'}/>{/if}
      <label>Board / Screen<select bind:value={profile} disabled={busy || installing || installOpen || !!identifiedBoard} required><option value="" disabled>Choose Board / Screen</option>{#each hostState.firmwareProfiles ?? [] as board}<option value={board.id}>{board.name}{board.available?'':' · firmware unavailable'}</option>{/each}</select></label>
    </div>
    <section class="firmware-controls" aria-label="Firmware">
      <h4>Firmware</h4>
      <p>{hostState.firmwareSource==='development'?'Development firmware from this Cockpit build':'Firmware from published ESP32 companion releases'}</p>
      {#if hostState.firmwareError}<CompanionMessage kind="warning" message={hostState.firmwareError}/>{/if}
      <button disabled={busy || installing || installOpen} onclick={()=>action(async()=>{await send('companion_firmware_check');})}><Icon name="refresh" size={16} />Check for updates</button>
      {#if !port}
        <CompanionMessage message="Select a port to detect firmware"/>
      {:else if detecting}
        <CompanionMessage message="Detecting firmware…"/>
      {:else}
        <CompanionMessage kind={firmwareCurrent?'info':'warning'} message={detectionError|| (firmwareCurrent?'Firmware is up to date':installedVersion?'Firmware update available':'Firmware not detected. Install the current firmware before pairing.')}/>
        <div class="firmware-versions">
          <p>Current: {installedVersion??'Not detected'}</p>
          {#if hostState.firmwareVersion}<p>Available: {hostState.firmwareVersion}</p>{/if}
        </div>
      {/if}
      {#if port && detectionError && !detecting && !installOpen}
        <button disabled={busy || installing} onclick={()=>action(()=>selectPort(port))}><Icon name="refresh" size={16} />Retry detection</button>
      {/if}
      {#if port && !detecting && !firmwareCurrent && !installOpen}
        <button class="firmware-action" disabled={busy || installing || !port || !hostState.firmwareProfiles?.find(p=>p.id===profile)?.available} onclick={()=>action(prepareInstall)}><Icon name="download" size={16} />{installedVersion?'Update firmware':'Install firmware'}</button>
      {:else if installOpen}

        <div class="firmware-install-actions">
          <button disabled={busy || installing} onclick={()=>action(async()=>{await send('companion_usb_flash',{port,profile});installing=true;})}><Icon name="download" size={16} />Install firmware</button>
          {#if installing || transport?.flashing}
            <div class="firmware-progress" role="progressbar" aria-label="Firmware installation" aria-valuemin="0" aria-valuemax="100" aria-valuenow={firmwareProgress}>
              <div class="firmware-progress-fill" style:width={firmwareProgress+'%'}></div><span>{firmwareProgress}%</span>
            </div>
          {:else}
            <button class="danger" disabled={busy} onclick={()=>action(async()=>{await selectPort(port);})}><Icon name="close" size={16} />Cancel</button>
          {/if}
        </div>
      {/if}
      {#if installing || usb?.flashing}<CompanionMessage message={`${transport?.stage||'Installing firmware'} · Keep the device connected.`}/>{/if}
    </section>
  </div>
  {#if step!==4}
    <section class="connection-setup" aria-label="Connection setup">
      <h4>Connection setup</h4>
      <div class="row connection-row">
        <label>Connection<select bind:value={mode} disabled={busy || installing}><option value="wifi">Wi-Fi</option><option value="serial">Serial</option></select></label>
        <label>Screen name<input bind:value={name} maxlength="80" disabled={busy || installing} /></label>
      </div>
      {#if mode==='wifi'}
        <div class="row device-row">
          <label>Wi-Fi network<select value={ssid} disabled={busy || installing} onchange={e=>ssid=e.currentTarget.value}><option value="">Choose a network</option>{#if ssid && !(usb?.networks??[]).some(n=>n.ssid===ssid)}<option value={ssid}>{ssid}</option>{/if}{#each [...new Map((usb?.networks??[]).map(n=>[n.ssid,n])).values()] as network}<option value={network.ssid}>{network.ssid} · {network.rssi} dBm{network.secured?' · secured':''}</option>{/each}</select></label>
          <button disabled={busy || installing || !!usb?.scanning || !firmwareCurrent} onclick={()=>action(async()=>{await send('companion_usb_scan');})}><Icon name="refresh" size={16} />{usb?.scanning?'Scanning…':'Scan'}</button>
        </div>
        {#if usb?.scanning}<CompanionMessage message="Scanning Wi-Fi networks…"/>{/if}
      <label>Network name (or hidden SSID)<input bind:value={ssid} maxlength="32" autocomplete="off" disabled={busy || installing} /></label>
        {#if ssid}<label>Wi-Fi password<input type="password" bind:value={password} oninput={()=>passwordChanged=true} placeholder={savedPassword && !passwordChanged && ssid===savedSsid?'•••••••• (saved on device)':'Enter Wi-Fi password'} maxlength="64" autocomplete="new-password" disabled={busy || installing} /></label>{/if}
        {#if hostState.listener && !hostState.listener.enabled}<CompanionMessage kind="warning" message="Enable the companion listener in Settings so the screen can reach this PC."/>{/if}
      {/if}
    </section>
    <section class="panel-setup" aria-label="Panel setup">
      <h4>Panel setup</h4>
      <label>Panel<select bind:value={selectedHash} disabled={busy || installing}>
        <option value="">Assign a panel later</option>
        {#each publications as panel}<option value={panel.publicationHash} title={panelScreenReason(panel,screenSize)} disabled={!!panelScreenReason(panel,screenSize)}>{panel.document.ui.title}{panelScreenReason(panel,screenSize)?' · Not compatible':''}</option>{/each}
      </select></label>
      <CompanionMessage message={screenSize?`Screen size: ${screenSize.width} × ${screenSize.height} px. Panels must match this size on every page.`:'Connect the screen to identify its size before assigning a panel.'}/>
    </section>
    <div class="pair-device-actions"><span class="disabled-reason" title={pairDisabledReason || undefined} aria-label={pairDisabledReason || undefined}>
      <button disabled={!!pairDisabledReason || installOpen} onclick={()=>action(provision)}><Icon name="link" size={16} />Pair device</button>
    </span></div>
  {:else}
    <CompanionMessage message="Pairing device. Keep USB connected until setup is saved."/>
  {/if}
</section>
<style>
 .usb button.danger{background:#9b2430;border-color:#bc3441;color:#fff}.usb button.danger:hover{background:#b32b38;border-color:#e35d69}
 .firmware-install-actions{display:flex;align-items:center;gap:16px;flex-wrap:wrap}.firmware-progress{position:relative;flex:1;min-width:160px;height:36px;margin-left:auto;overflow:hidden;background:#101c27;border:1px solid #456170;border-radius:5px}.firmware-progress-fill{height:100%;background:#326c58;transition:width .2s}.firmware-progress span{position:absolute;inset:0;display:grid;place-items:center;color:#fff;font-weight:600}.connection-setup,.panel-setup{border-top:1px solid #344452;margin-top:24px;padding-top:4px}.connection-row label{flex:1;min-width:180px}.connection-setup .device-row{max-width:600px}
 .firmware-versions{display:grid;gap:6px;margin-top:12px}.firmware-versions p{margin:0}
 .setup-columns{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:28px;margin-top:22px}.device-controls{min-width:0}.firmware-controls{min-width:0;border-left:1px solid #344452;padding-left:28px}.firmware-controls h4{margin:0 0 10px}.firmware-controls .firmware-action{margin-top:16px}.device-row{align-items:flex-end;margin-top:0}.device-row label{flex:1;min-width:180px;margin:0}.device-row button{margin:0}.device-controls>label{max-width:none}.pair-device-actions{display:flex;justify-content:flex-end;margin-top:24px}.disabled-reason{display:inline-flex}.disabled-reason:has(button:disabled){cursor:not-allowed}.disabled-reason button:disabled{pointer-events:none}@media(max-width:850px){.setup-columns{grid-template-columns:1fr}.firmware-controls{border-left:0;border-top:1px solid #344452;padding-left:0;padding-top:20px}}

 .usb{border:1px solid #344452;border-radius:8px;padding:16px;margin:14px 0;color:#dce7ed;font-size:13px}header{display:flex;justify-content:space-between;gap:16px;align-items:start}h3{margin:0}h4{margin:18px 0 10px}p{line-height:1.5;color:#a9bdcb}.row{display:flex;gap:10px;align-items:end;flex-wrap:wrap;margin:12px 0}label{display:flex;flex-direction:column;gap:6px;margin:10px 0;max-width:500px}button,input,select{font:inherit;color:#dce7ed;border:1px solid #344452;border-radius:5px;background:#14212c;padding:9px}button{cursor:pointer;margin:3px;display:inline-flex;align-items:center;gap:6px}button:disabled{opacity:.45;cursor:default}
</style>
