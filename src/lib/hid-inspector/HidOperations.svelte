<script lang="ts">
  import Icon from '../Icon.svelte';
  import type {HidInspectionController,HidInspectionState} from '../controller-hid';
  import {exact,MAX_IMPORT_BYTES,object} from './model';
  let {controller,view,clearHistory}:{controller:HidInspectionController;view:HidInspectionState;clearHistory:()=>void}=$props();
  let playing=$state(false),importError=$state(''),seekUs=$state('0');
  let playStarted=0,playOrigin=0;
  let getKind=$state<'input'|'feature'>('input'),reportId=$state(0),maxBytes=$state(65536),deadlineMs=$state(500),recordMiB=$state(8),recordCount=$state(10000);
  const recording=$derived(object(view.recording?.result));
  $effect(()=>{
    if(!playing||!view.replay||view.busy)return;
    const replay=view.replay;
    if(replay.complete){playing=false;return;}
    const timer=setTimeout(()=>{void controller.replayRead(String(replay.next??'0'),String(Math.floor(playOrigin+(performance.now()-playStarted)*1000)));},50);
    return()=>clearTimeout(timer);
  });
  async function importFile(event:Event){
    const input=event.currentTarget as HTMLInputElement,file=input.files?.[0];if(!file)return;
    importError='';playing=false;
    if(file.size>MAX_IMPORT_BYTES){importError='Recording exceeds the 64 MiB import limit. Export a smaller bounded capture.';input.value='';return;}
    try{clearHistory();await controller.importRecording(await file.text());if(controller.state.replay)await controller.replayRead('0','0',128,true);}catch(error){importError=String(error);}finally{input.value='';}
  }
  async function exportFile(){
    const contents=await controller.exportRecording();if(!contents)return;
    const url=URL.createObjectURL(new Blob(contents,{type:'application/json'})),link=document.createElement('a');
    link.href=url;link.download='hid-recording.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  }
  function seek(){playing=false;clearHistory();void controller.replayRead('0',seekUs,128,true);}
  function togglePlay(){
    if(playing){playing=false;return;}
    playOrigin=Number(view.replay?.positionUs??0);playStarted=performance.now();playing=true;
  }
</script>
<label>Retained recording<select aria-label="Retained HID recording" disabled={view.busy} value={view.recordingTarget?JSON.stringify(view.recordingTarget):''} onchange={event=>{if(!event.currentTarget.value)return;const identity=JSON.parse(event.currentTarget.value);void controller.selectRecording(identity.interfaceId,identity.generation);}}><option value="">Select a retained recording</option>{#each view.recordings as archive,index (index)}<option value={JSON.stringify({interfaceId:archive.interfaceId,generation:archive.generation})}>{exact(archive.interfaceId)} · {exact(archive.generation)} · {exact(archive.status)}</option>{/each}</select></label>
{#if view.session||view.recordingTarget}
  <section aria-label="Bounded HID recording"><h3>Recording</h3><p>Records independently of display refresh. Capacity stops with an explicit loss marker. Stop and export, or discard to release storage.</p>
    <div class="controls"><label>Memory limit (MiB)<input aria-label="HID recording memory limit" type="number" min="1" max="64" bind:value={recordMiB}/></label><label>Record limit<input aria-label="HID recording record limit" type="number" min="3" max="1000000" bind:value={recordCount}/></label></div>
    <div class="actions"><button class="secondary-button" aria-label="Start recording" disabled={view.busy||!!view.recording||!view.session} onclick={()=>controller.record('start',Math.min(64,Math.max(1,recordMiB))*1024*1024,recordCount)}><Icon name="activity" size={14}/>Start</button><button class="secondary-button" aria-label="Stop recording" disabled={view.busy||recording.status!=='recording'} onclick={()=>controller.record('stop')}><Icon name="close" size={14}/>Stop</button><button class="secondary-button" disabled={view.busy} onclick={()=>controller.record('status')}><Icon name="refresh" size={14}/>Check status</button><button class="secondary-button" disabled={view.busy||recording.status!=='stopped'} onclick={exportFile}><Icon name="save" size={14}/>Export</button><button class="secondary-button" disabled={view.busy||recording.status!=='stopped'} onclick={()=>controller.record('discard')}><Icon name="trash" size={14}/>Discard</button></div>
    {#if view.recording}<p role="status">Recording status: {exact(recording.status)} · {exact(recording.recordCount)} records · {exact(recording.bytes)} bytes · terminal / loss outcome: {exact(recording.reason)}</p>{/if}
  </section>
{/if}
{#if view.session}
  <section aria-label="Read-only HID requests"><h3>Read-only report request</h3><p>Input and Feature GET support is platform and device dependent. Native policy and declared report bounds remain authoritative; the outcome below reports actual support.</p>
    <div class="controls"><label>Kind<select aria-label="HID GET kind" bind:value={getKind}><option value="input">Input</option><option value="feature">Feature</option></select></label><label>ID (0 = unnumbered)<input aria-label="HID GET report ID" type="number" min="0" max="255" bind:value={reportId}/></label><label>Maximum bytes<input aria-label="HID GET byte limit" type="number" min="1" max="65536" bind:value={maxBytes}/></label><label>Deadline (ms)<input aria-label="HID GET deadline" type="number" min="1" max="2000" bind:value={deadlineMs}/></label></div>
    <button class="secondary-button" disabled={view.busy} onclick={()=>controller.get(getKind,reportId,maxBytes,deadlineMs)}><Icon name="refresh" size={14}/>Request</button>
    {#if view.getResult}<pre role="status">{JSON.stringify(view.getResult,null,2)}</pre>{/if}
  </section>
{/if}
<section aria-label="Offline HID replay"><h3>Offline replay</h3><label>Import recording · JSON, maximum 64 MiB<input aria-label="Import HID recording" type="file" accept="application/json,.json" disabled={view.busy} onchange={importFile}/></label>
  {#if importError}<p role="alert">{importError}</p>{/if}
  {#if view.replay}<p role="status">Imported observations · {playing?'playing at original acquisition timing':'paused'} · source generation {exact(object(view.replay.metadata).generation)} · position {exact(view.replay.positionUs??'0')} / {exact(view.replay.durationUs??object(view.replay.metadata).durationUs)} µs. Replay does not inject inputs or send output reports.</p>
    <div class="actions"><button class="secondary-button" aria-label={playing?'Pause replay':'Play replay'} disabled={view.busy} onclick={togglePlay}><Icon name="activity" size={14}/>{playing?'Pause':'Play'}</button><button class="secondary-button" disabled={view.busy} onclick={()=>{playing=false;void controller.replayRead(String(view.replay?.next??'0'),undefined,1);}}><Icon name="arrow" size={14}/>Step</button><button class="secondary-button" disabled={view.busy} onclick={()=>{seekUs='0';seek();}}><Icon name="refresh" size={14}/>Rewind</button><button class="secondary-button" aria-label="Close replay" disabled={view.busy} onclick={()=>{playing=false;void controller.select('');}}><Icon name="close" size={14}/>Close</button></div>
    <div class="controls"><label>Seek acquisition time (µs)<input aria-label="HID replay seek time" inputmode="numeric" bind:value={seekUs}/></label><button class="secondary-button" disabled={view.busy||!/^\d+$/.test(seekUs)} onclick={seek}><Icon name="search" size={14}/>Seek</button></div>
  {/if}
</section>
<style>
  h3{font-size:var(--ui-heading-size);margin:20px 0 8px}p,label{color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}label{display:grid;gap:8px}input,select{width:100%;min-height:var(--ui-control-height);font-size:var(--ui-text-size)}.actions,.controls{display:flex;flex-wrap:wrap;gap:8px;margin:12px 0}.controls{align-items:end}.controls label{max-width:220px}button{display:inline-flex;align-items:center;gap:6px}pre{max-height:260px;overflow:auto;font-size:var(--ui-text-size);white-space:pre-wrap;overflow-wrap:anywhere}@media(max-width:600px){.controls label{max-width:100%}}
</style>
