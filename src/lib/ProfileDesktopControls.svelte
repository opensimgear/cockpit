<script lang="ts">
  import{onMount,untrack}from'svelte';
  import{gatewayRequest}from'./gateway';
  import{desktopStatus,type DesktopStatus,type DesktopRequest,type DesktopScope}from'./profile-desktop-runtime';
  let{active=true,requested=false}:{active?:boolean;requested?:boolean}=$props();
  let status=$state.raw<DesktopStatus|null>(null),busy=$state(''),error=$state(''),mounted=$state(false),requestEpoch=0;
  async function request(request:DesktopRequest){const epoch=++requestEpoch;busy=request.command;error='';try{const captured=desktopStatus((await gatewayRequest(request)).desktop);if(epoch===requestEpoch)status=captured;}catch(reason){if(epoch===requestEpoch)error=reason instanceof Error?reason.message:String(reason);}finally{if(epoch===requestEpoch)busy='';}}
  onMount(()=>{mounted=true;return()=>{mounted=false;requestEpoch++;};});
  $effect(()=>{if(!mounted||!active)return;const refresh=()=>{if(!document.hidden&&!busy)void request({command:'profile_desktop_status'});};untrack(refresh);const timer=window.setInterval(refresh,1500);return()=>window.clearInterval(timer);});
  function configure(enabled:boolean,scope:DesktopScope){void request({command:'profile_desktop_configure',enabled,scope});}
  const runtime=$derived(status?.runtime&&typeof status.runtime==='object'?status.runtime as Record<string,unknown>:null);
  const applicationName=$derived.by(()=>{if(!status?.application||typeof status.application!=='object')return 'Choose an application, or Apply with a verified game context';const app=status.application as Record<string,unknown>;const path=typeof app.executable==='string'?app.executable:typeof app.path==='string'?app.path:'';return path.split(/[\\/]/).at(-1)||'Selected native application';});
</script>
<section class="desktop-controls panel" aria-label="Native keyboard and mouse output">
  <div class="heading"><strong>Keyboard and mouse output</strong><span aria-live="polite">{status?.state??'Waiting for native status'}</span><button class="text-button" disabled={!!busy} onclick={()=>request({command:'profile_desktop_status'})}>Refresh status</button></div>
  <p>{requested?'This draft requires keyboard and mouse output.':'Add a keyboard and mouse endpoint to use desktop output.'} Desktop input has independent local enablement and qualification. A desktop-only graph needs no virtual controller or hiding driver.</p>
  {#if status}
    <div class="settings"><label class="check"><input type="checkbox" checked={status.enabled} disabled={!!busy||!!status.error} onchange={event=>configure(event.currentTarget.checked,status!.scope)}/>Enable desktop output on this computer</label><label>Application scope<select value={status.scope.kind} disabled={!!busy||!!status.error} onchange={event=>configure(status!.enabled,{kind:event.currentTarget.value as DesktopScope['kind']})}><option value="application">Selected application (default)</option><option value="desktop-wide">Desktop-wide navigation</option></select></label><button class="secondary-button" disabled={!!busy||!!status.error} onclick={()=>request({command:'profile_desktop_capture_application'})}>Use last focused external application</button></div>
    <dl><div><dt>Qualification</dt><dd>{status.qualified?'Development qualification build':'Awaiting Windows qualification'}</dd></div><div><dt>Selected application</dt><dd>{applicationName}</dd></div></dl>
    {#if status.state==='waiting'}<p role="status">Release or center the waiting controls to rearm them. Actions already armed can continue.</p>{/if}
    {#if status.reason}<p role="status">{status.reason}</p>{/if}{#if status.error}<p class="error" role="alert">{status.error}</p>{/if}
    {#if runtime}<details><summary>Native owners, operations and cleanup</summary><p>Submission acknowledgements describe the OS API result. Verify receiver behavior separately.</p><pre>{JSON.stringify(runtime,null,2)}</pre></details>{/if}
  {/if}
  <div class="actions"><button class="secondary-button" disabled={busy==='profile_desktop_stop'} onclick={()=>request({command:'profile_desktop_stop'})}>Stop keyboard and mouse output</button><button class="text-button" disabled={!!busy} onclick={()=>request({command:'profile_desktop_recover'})}>Recover desktop outputs</button></div>
  <p>Stop cancels pending work across the active profile and persists Pause against automatic rearming. Saving or changing scope never starts output; Apply the saved revision explicitly. Focus loss clears holds and requires neutral controls before rearming.</p>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>
<style>
 .desktop-controls{padding:17px;margin:0 0 22px}.heading,.settings,.actions{display:flex;align-items:center;gap:12px;flex-wrap:wrap}.heading strong{font-size:12px}.heading span{font-size:10px;color:var(--accent)}.heading button{margin-left:auto}.settings{align-items:end}.settings label{display:grid;gap:6px;font-size:10px}.settings label.check{display:flex;align-items:center;gap:7px}p{font-size:10px;line-height:1.8;color:var(--muted);margin:12px 0}button,select{font-size:10px}select{padding:7px;background:var(--input-bg);color:var(--text);border:1px solid var(--line);border-radius:5px}dl{display:flex;gap:24px;flex-wrap:wrap}dt{font-size:9px;color:var(--muted)}dd{margin:6px 0;font-size:10px;overflow-wrap:anywhere}details{font-size:10px;margin:15px 0}pre{overflow:auto;max-height:300px;font-size:10px;line-height:1.7}.error{color:#d3a078}
</style>
