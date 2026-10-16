<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  let { disabled=false, onsubmitted }: { disabled?: boolean; onsubmitted: () => void } = $props();
  let name=$state('Companion'), code=$state(''), endpoint=$state(''), busy=$state(false), error=$state('');
  let hints=$state<{name:string;hostname:string;port:number;addresses:string[]}[]>([]);
  let video = $state<HTMLVideoElement>();
  let stream: MediaStream | undefined;
  let scanning=$state(false);
  let scanTimer: ReturnType<typeof setTimeout>;
  let generation=0;
  const request=<T,>(value: unknown) => invoke<T>('companion_client_request',{request:value});
  async function discover() {
    busy=true; error='';
    try { const result=await request<{hints:typeof hints}>({type:'discover',durationMs:1200}); hints=result.hints;
      if (!endpoint && hints[0]) endpoint=hostEndpoint(hints[0]);
    } catch(e) {error=String(e);} finally {busy=false;}
  }
  function hostEndpoint(host: typeof hints[number]) {
    const address=host.addresses.find(a=>!a.includes(':')) || host.hostname.replace(/\\.$/,'');
    return 'wss://'+address+':'+host.port+'/companion';
  }
  function stopScan() { generation++; scanning=false; clearTimeout(scanTimer); stream?.getTracks().forEach(t=>t.stop()); stream=undefined; }
  async function submit(payload?: string) {
    busy=true; error='';
    try {
      await request(payload?{type:'pair-start',payload,name}:{type:'pair-code-start',code,endpoint,name});
      code=''; stopScan(); onsubmitted();
    } catch(e) {error=String(e);} finally {busy=false;}
  }
  async function scan() {
    error='';
    const token=++generation;
    try {
      const Detector=(window as unknown as {BarcodeDetector?:new(options:{formats:string[]})=>{detect(source:HTMLVideoElement):Promise<{rawValue:string}[]>}}).BarcodeDetector;
      if (!Detector) throw new Error('QR scanning is unavailable on this device. Enter the pairing code instead.');
      stream=await navigator.mediaDevices.getUserMedia({video:{facingMode:'environment'},audio:false});
      if (token!==generation) {stream.getTracks().forEach(t=>t.stop());return;}
      scanning=true;
      await tick(); if(!video) throw new Error("Camera preview unavailable"); video.srcObject=stream; await video.play();
      const detector=new Detector({formats:['qr_code']});
      const read=async () => {
        if(token!==generation)return;
        try {const results=await detector.detect(video!);if(results[0]) {await submit(results[0].rawValue);return;}}
        catch(e){error=String(e);stopScan();return;}
        scanTimer=setTimeout(read,200);
      }; void read();
    } catch(e) {error=String(e);stopScan();}
  }
  onDestroy(stopScan);
</script>
<div class="pairing-form">
  <p>In Cockpit, choose Companions → Pair. Scan its QR code or enter its eight-character pairing code.</p>
  <label>This device name<input bind:value={name} maxlength="80" /></label>
  <button disabled={disabled||busy} onclick={scan}>Scan pairing QR code</button>
  {#if scanning}<video bind:this={video} autoplay playsinline muted aria-label="Pairing QR scanner"></video><button onclick={stopScan}>Stop scanning</button>{/if}
  <div class="host-row"><label>Cockpit host{#if hints.length}<select value={endpoint} onchange={e=>endpoint=e.currentTarget.value}>{#each hints as host}<option value={hostEndpoint(host)}>{host.name}</option>{/each}</select>{/if}<input bind:value={endpoint} placeholder="wss://192.168.1.10:54437/companion" /></label><button disabled={disabled||busy} onclick={discover}>Find nearby hosts</button></div>
  <label>Pairing code<input bind:value={code} maxlength="8" placeholder="ABCDEFGH" autocapitalize="characters" autocomplete="off" spellcheck="false" /></label>
  <button disabled={disabled||busy||code.trim().length!==8||!endpoint||!name.trim()} onclick={()=>submit()}>Pair with Cockpit</button>
  {#if error}<p role="alert">{error}</p>{/if}
</div>
<style>
  .pairing-form{display:grid;gap:12px}.pairing-form p{font-size:12px;line-height:1.7}.pairing-form label{display:grid;gap:6px;font-size:12px}.pairing-form input,.pairing-form select,.pairing-form button{font:inherit;font-size:12px;padding:10px;border:1px solid #344452;border-radius:6px;background:#14212c;color:#e1eaf0;min-width:0}.pairing-form button{cursor:pointer}.pairing-form button:disabled{opacity:.4}.host-row{display:flex;align-items:end;gap:8px}.host-row label{flex:1}video{width:100%;max-height:260px;object-fit:contain}p[role=alert]{color:#ed8383}
</style>
