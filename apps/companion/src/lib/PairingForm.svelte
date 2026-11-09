<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { scanQrFrames } from './qr-frame-scan';
  let { disabled=false, onsubmitted }: { disabled?: boolean; onsubmitted: () => void } = $props();
  let name=$state('Companion'), code=$state(''), endpoint=$state(''), busy=$state(false), error=$state('');
  let hints=$state<{name:string;endpoint:string}[]>([]);
  let video = $state<HTMLVideoElement>();
  let stream: MediaStream | undefined;
  let scanning=$state(false);
  let stopFrameScan: (() => void) | undefined;
  let generation=0;
  const request=<T,>(value: unknown) => invoke<T>('companion_client_request',{request:value});
  async function discover() {
    busy=true; error='';
    try { const result=await request<{hints:typeof hints}>({type:'discover',durationMs:1200}); hints=result.hints;
      if (!endpoint && hints[0]) endpoint=hints[0].endpoint;
    } catch(e) {error=String(e);} finally {busy=false;}
  }
  function stopScan() { generation++; scanning=false; stopFrameScan?.(); stopFrameScan=undefined; stream?.getTracks().forEach(t=>t.stop()); stream=undefined; }
  async function submit(payload?: string) {
    stopScan();
    busy=true; error='';
    try {
      await request(payload?{type:'pair-start',payload,name}:{type:'pair-code-start',code,endpoint,name});
      code=''; onsubmitted();
    } catch(e) {error=String(e);} finally {busy=false;}
  }
  async function scan() {
    error='';
    stopScan();
    const token=generation;
    try {
      const Detector=(window as unknown as {BarcodeDetector?:new(options:{formats:string[]})=>{detect(source:HTMLVideoElement):Promise<{rawValue:string}[]>}}).BarcodeDetector;
      if (!Detector || typeof HTMLVideoElement.prototype.requestVideoFrameCallback !== 'function'
        || typeof HTMLVideoElement.prototype.cancelVideoFrameCallback !== 'function') {
        throw new Error('QR scanning is unavailable on this device. Enter the pairing code instead.');
      }
      const acquired = await navigator.mediaDevices.getUserMedia({video:{facingMode:'environment'},audio:false});
      if (token !== generation) {
        acquired.getTracks().forEach(track => track.stop());
        return;
      }
      stream = acquired;
      scanning = true;
      await tick();
      if (token !== generation) return;
      const preview = video;
      if (!preview) throw new Error('Camera preview unavailable');
      preview.srcObject = acquired;
      await preview.play();
      if (token !== generation) return;
      const detector = new Detector({formats:['qr_code']});
      stopFrameScan = scanQrFrames(preview,
        async () => (await detector.detect(preview))[0]?.rawValue,
        value => { void submit(value); },
        reason => { error = String(reason); stopScan(); });
    } catch (reason) {
      if (token !== generation) return;
      error = String(reason);
      stopScan();
    }
  }
  onDestroy(stopScan);
</script>
<svelte:document onvisibilitychange={() => { if (document.hidden) stopScan(); }}/>
<div class="pairing-form">
  <p>In Cockpit, choose Companions → Pair. Scan its QR code or enter its eight-character pairing code.</p>
  <label>This device name<input bind:value={name} maxlength="80" /></label>
  <button disabled={disabled||busy} onclick={scan}>Scan pairing QR code</button>
  {#if scanning}<video bind:this={video} autoplay playsinline muted aria-label="Pairing QR scanner"></video><button onclick={stopScan}>Stop scanning</button>{/if}
  <div class="host-row"><label>Cockpit host{#if hints.length}<select value={endpoint} onchange={e=>endpoint=e.currentTarget.value}>{#each hints as host}<option value={host.endpoint}>{host.name}</option>{/each}</select>{/if}<input bind:value={endpoint} placeholder="wss://192.168.1.10:54437/companion" /></label><button disabled={disabled||busy} onclick={discover}>Find nearby hosts</button></div>
  <label>Pairing code<input bind:value={code} maxlength="8" placeholder="ABCDEFGH" autocapitalize="characters" autocomplete="off" spellcheck="false" /></label>
  <button disabled={disabled||busy||code.trim().length!==8||!endpoint||!name.trim()} onclick={()=>submit()}>Pair with Cockpit</button>
  {#if error}<p role="alert">{error}</p>{/if}
</div>
<style>
  .pairing-form{display:grid;gap:12px}.pairing-form p{font-size:12px;line-height:1.7}.pairing-form label{display:grid;gap:6px;font-size:12px}.pairing-form input,.pairing-form select,.pairing-form button{font:inherit;font-size:12px;padding:10px;border:1px solid #344452;border-radius:6px;background:#14212c;color:#e1eaf0;min-width:0}.pairing-form button{cursor:pointer}.pairing-form button:disabled{opacity:.4}.host-row{display:flex;align-items:end;gap:8px}.host-row label{flex:1}video{width:100%;max-height:260px;object-fit:contain}p[role=alert]{color:#ed8383}
</style>
