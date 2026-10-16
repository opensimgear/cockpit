<script lang="ts">
  import { onMount } from 'svelte';
  import CompanionFeedback from './CompanionFeedback.svelte';
  import { gatewayRequest } from './gateway';
  import { sendPublication, stableCompanionId, type CompanionCommand, type CompanionHostState } from './companions';
  import type { Publication } from '@opensimgear/companion-designer';
  let { publications = [] }: { publications?: Publication[] } = $props();
  let hostState = $state.raw<CompanionHostState>({});
  let address = $state('127.0.0.1');
  let advertisedHost = $state('127.0.0.1');
  let port = $state(54437);
  let discovery = $state(true);
  let selectedHash = $state('');
  let companionId = $state('');
  let instanceId = $state<string>(`panel-${crypto.randomUUID()}`);
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let pairing = $state<CompanionHostState['pairing']>();
  let expires = $state(0);
  let secondsLeft = $state(0);
  let polling = false;
  const revision = $derived(publications.find(p => p.publicationHash === selectedHash) ?? publications.at(-1));
  const text = (row: Record<string, unknown>, key: string) => typeof row[key] === 'string' ? String(row[key]) : '';
  const id = (row: Record<string, unknown>) => text(row, 'companionId');
  async function send(command: CompanionCommand, companion: Record<string, unknown> = {}) {
    const result = (await gatewayRequest({ command, companion })).companions;
    if (!result) throw new Error('Companion connections require the updated native Cockpit app.');
    hostState = { ...hostState, ...result }; return result;
  }
  async function action(work: () => Promise<void>) {
    if (busy) return; busy = true; error = ''; notice = '';
    try { await work(); } catch (e) { error = String(e); } finally { busy = false; }
  }
  async function refresh() {
    if (busy || polling) return; polling = true;
    try { await send('companion_status'); } catch (e) { error = String(e); } finally { polling = false; }
  }
  async function offer() {
    const result = await send('companion_pair_offer'); pairing = result.pairing;
    expires = Date.now() + (pairing?.ttlMs ?? 0); secondsLeft = Math.ceil((pairing?.ttlMs ?? 0) / 1000);
  }
  onMount(() => {
    const timer = setInterval(() => {
      secondsLeft = Math.max(0, Math.ceil((expires - Date.now()) / 1000)); if (!secondsLeft) pairing = undefined;
      void refresh();
    }, 1000);
    return () => { clearInterval(timer); pairing = undefined; };
  });
</script>

<details class="connections" ontoggle={e => { if (e.currentTarget.open) void refresh(); }}>
  <summary>Companion connections · {hostState.listener?.enabled ? 'Listening' : 'Disabled'}</summary>
  <p>Connect a Windows, Linux or macOS desktop to this host. Android and iOS pairing is not available yet. Pairing and assigning a panel leave game output inactive; start a mapping separately when its capabilities are qualified.</p>
  {#if error}<p class="error" role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}
  <div inert={busy}>
    <div class="row"><label>Listen on IP<input bind:value={address} placeholder="192.168.1.10" /></label><label>Advertised host IP<input bind:value={advertisedHost} /></label><label>Port<input type="number" min="1024" max="65535" bind:value={port} /></label><label><input type="checkbox" bind:checked={discovery} />LAN discovery</label>
      <button disabled={!!hostState.listener?.enabled} onclick={() => action(async () => { await send('companion_enable', { address, advertisedHost, port, discovery }); notice = 'Listener enabled. Pair and assign each installation explicitly.'; })}>Enable listener</button>
      <button disabled={!hostState.listener?.enabled} onclick={() => action(async () => { await send('companion_disable'); pairing = undefined; notice = 'Listener disabled; companion sources are neutral.'; })}>Disable listener</button><button onclick={refresh}>Refresh</button>
    </div>
    {#if hostState.interfaces?.length}<p class="small">Available addresses: {hostState.interfaces.join(', ')}</p>{/if}
    {#if hostState.listener?.endpoint}<p>Endpoint: <code>{hostState.listener.endpoint}</code></p>{/if}
    {#if hostState.listener?.caSpkiSha256}<p class="small">Host fingerprint: <code>{hostState.listener.caSpkiSha256}</code></p>{/if}
    {#if hostState.listener?.lastError}<p class="error">{hostState.listener.lastError}</p>{/if}
    <button disabled={!hostState.listener?.enabled} onclick={() => action(offer)}>Pair a companion</button>
    {#if pairing && secondsLeft}
      <div class="pairing"><div><h4>One-time pairing offer · {secondsLeft}s</h4><p>Scan from the companion or copy this offer to its pairing screen. The offer includes the host fingerprint; share it only with the installation you intend to pair.</p><textarea aria-label="One-time pairing offer" readonly rows="4" value={pairing.manualPayload}></textarea></div>
        {#if pairing.qrMatrix.length}<svg aria-label="Companion pairing QR code" role="img" viewBox={`-4 -4 ${pairing.qrMatrix.length+8} ${pairing.qrMatrix.length+8}`}><rect x="-4" y="-4" width={pairing.qrMatrix.length+8} height={pairing.qrMatrix.length+8} fill="white" />{#each pairing.qrMatrix as row,y}{#each row as dark,x}{#if dark === '1'}<rect {x} {y} width="1" height="1" fill="black" />{/if}{/each}{/each}</svg>{/if}
      </div>
    {/if}
    {#each hostState.host?.pending ?? [] as pending}<div class="card"><span>Pairing request: {text(pending,'displayName') || text(pending,'name') || id(pending)}</span><button onclick={() => action(async () => { await send('companion_approve', { companionId: id(pending), requestId: text(pending,'requestId') }); notice = 'Installation approved. Assign a published panel next.'; })}>Approve installation</button></div>{/each}
    <h4>Published panels</h4>
    <div class="row"><label>Local revision<select value={revision?.publicationHash ?? ''} onchange={e => selectedHash = e.currentTarget.value}>{#if !publications.length}<option value="">Publish a draft above first</option>{/if}{#each publications as item}<option value={item.publicationHash}>{item.document.ui.title} · {item.publicationHash.slice(7,19)}</option>{/each}</select></label><button disabled={!revision} onclick={() => action(async () => { await sendPublication(revision!,send); notice = 'Verified immutable revision is available to assigned companions.'; })}>Make revision available</button></div>
    <h4>Controller assignments</h4><div class="row"><label>Installation<select bind:value={companionId}><option value="">Choose a paired companion</option>{#each hostState.host?.clients ?? [] as client}<option value={id(client)}>{text(client,'displayName') || text(client,'name') || id(client)}</option>{/each}</select></label><label>Controller instance ID<input bind:value={instanceId} maxlength="64" /></label><button disabled={!companionId || !revision} onclick={() => action(async () => { await send('companion_assign', { companionId: stableCompanionId(companionId), instanceId: stableCompanionId(instanceId), publicationHash: revision!.publicationHash }); notice = 'Assignment saved. It remains disarmed until the companion starts a fresh session.'; })}>Assign panel</button></div>
    {#each hostState.host?.instances ?? [] as instance}<div class="card"><span>{text(instance,'instanceId')} · {text(instance,'companionId')} · {instance.armed ? 'Input session armed' : 'Disarmed'}</span><button onclick={() => action(async () => { await send('companion_disconnect', { instanceId: text(instance,'instanceId') }); })}>Disconnect input</button><button disabled={!companionId} onclick={() => action(async () => { await send('companion_takeover', { instanceId: text(instance,'instanceId'), companionId: stableCompanionId(companionId) }); notice = 'Prior writer neutralized. The replacement must establish a fresh baseline.'; })}>Give selected installation control</button></div>{/each}
    <CompanionFeedback {send} />
    <h4>Paired installations</h4>{#each hostState.host?.clients ?? [] as client}<div class="card"><span>{text(client,'displayName') || text(client,'name') || id(client)} <code>{id(client)}</code></span><button onclick={() => action(async () => { await send('companion_revoke', { companionId: id(client) }); notice = 'Credential revoked and its owned sources neutralized.'; })}>Revoke access</button></div>{/each}
  </div>
</details>

<style>
  .connections{border:1px solid #344452;border-radius:7px;padding:12px;margin:14px 0;color:#dce7ed;font-size:13px}summary{cursor:pointer;font-weight:600}p{color:#9aafbf;line-height:1.5}.row{display:flex;gap:10px;align-items:end;flex-wrap:wrap}label{display:flex;flex-direction:column;gap:5px;font-size:12px}button,input,select,textarea{font:inherit;color:#dce7ed;border:1px solid #344452;border-radius:5px;background:#14212c;padding:8px}button{cursor:pointer}button:disabled{opacity:.4;cursor:default}textarea{width:100%;box-sizing:border-box;font-size:11px}.small{font-size:11px;overflow-wrap:anywhere}code{overflow-wrap:anywhere}.pairing{display:grid;grid-template-columns:1fr 240px;gap:20px;margin:18px 0}.pairing svg{width:240px;background:white;shape-rendering:crispEdges}.card{display:flex;justify-content:space-between;gap:12px;align-items:center;border-top:1px solid #344452;padding:10px 0}.error{color:#ffb7bb}@media(max-width:700px){.pairing{display:block}.card{flex-wrap:wrap}}
</style>
