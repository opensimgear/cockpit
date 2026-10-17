<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import Notification from './Notification.svelte';
  import { gameIntegrationUserFolder, type Game, type GameRequest, type GamesState } from './games';
  import { prepareProviderSettingsSave, providerSettingsDraft, providerSettingsEqual, providerSettingsError, settleProviderSettingsSave, type DcsConnection, type ProviderSettingsDraft, type XPlaneConnection } from './game-provider-settings';
  let { game, busy, userFolder = $bindable(''), request }: { game: Game; busy: boolean; userFolder?: string; request: (request: GameRequest) => Promise<GamesState | false> } = $props();
  function settingsForGame(value: Game): ProviderSettingsDraft { return { ...providerSettingsDraft(value), userFolder: value.id === 'dcs' ? gameIntegrationUserFolder(value) : '' }; }
  let base = $state<ProviderSettingsDraft>(settingsForGame(untrack(() => game)));
  let draft = $state<ProviderSettingsDraft | null>(settingsForGame(untrack(() => game)));
  let saving = $state(false), notice = $state('');
  let disposed = false;
  onDestroy(() => { disposed = true; });
  const saved = $derived(settingsForGame(game));
  const dirty = $derived(!!draft && !providerSettingsEqual(draft, base));
  const error = $derived(draft ? providerSettingsError(draft) : '');
  const dcs = $derived(draft?.gameId === 'dcs' ? draft.connection as DcsConnection : null);
  const xplane = $derived(draft?.gameId.startsWith('x-plane-') ? draft.connection as XPlaneConnection : null);
  const remoteChanged = $derived(!!draft && draft.baseRevision !== saved.baseRevision);
  $effect(() => {
    if (!draft || draft.gameId !== game.id || !dirty && !saving && remoteChanged) { draft = settingsForGame(game); base = settingsForGame(game); }
  });
  // Share only the chosen installer folder; provider fields retain their own draft/base.
  $effect(() => { if (game.id === 'dcs' && draft) userFolder = draft.userFolder; });
  function discard() { draft = settingsForGame(game); base = settingsForGame(game); notice = ''; }
  async function save() {
    if (!draft || saving || busy || error) return;
    const captured = prepareProviderSettingsSave(draft);
    const editor = draft;
    saving = true; notice = '';
    try {
      const result = await request(captured.request);
      if (disposed || !draft || draft.gameId !== captured.gameId) return;
      if (!result || result.error) throw new Error(result && result.error || 'Game settings were not saved. Refresh and try again.');
      const updated = result.catalogue.find(item => item.id === captured.gameId);
      if (!updated) throw new Error('The response did not include this game. Refresh before saving again.');
      // A discard/reopen while saving owns a different editor, even for the same game.
      if (draft !== editor) return;
      const current = draft;
      draft = settleProviderSettingsSave(current, captured, updated);
      base = settingsForGame(updated);
      notice = providerSettingsEqual(draft, providerSettingsDraft(updated)) ? 'Game integration settings saved.' : 'Settings saved. Your newer edits are still unsaved.';
    } catch (error) { if (!disposed) notice = error instanceof Error ? error.message : String(error); }
    finally { if (!disposed) saving = false; }
  }
</script>

<section class="provider-settings" aria-label={`${game.name} provider settings`}>
  <div class="settings-heading"><div><h3>{game.id === 'dcs' ? 'DCS-BIOS connection and catalogue' : 'Provider connection'}</h3><p>Saved for this game. Profile graphs choose commands, channels and processing.</p></div><span class="settings-state">{dirty ? 'Unsaved changes' : 'Saved settings'}</span></div>
  {#if draft}
    {#if dcs}
      {#if game.integrationSetup?.candidates?.length}<label class="wide">Detected Saved Games folders<select aria-label="Detected DCS Saved Games folders" value={game.integrationSetup.candidates.includes(draft.userFolder) ? draft.userFolder : ''} onchange={event => { if (draft) draft.userFolder = event.currentTarget.value; }}><option value="">Choose a folder or enter a path below</option>{#each game.integrationSetup.candidates as folder (folder)}<option value={folder}>{folder}</option>{/each}</select></label>{/if}
      <label class="wide">DCS Saved Games folder<input class="text-input" aria-label="DCS Saved Games folder" bind:value={draft.userFolder} placeholder="C:\Users\…\Saved Games\DCS" /></label>
      <p class="help wide">Setup installs DCS-BIOS and connects its export script in this folder. Saving settings installs nothing.</p>
      <label class="wide">DCS-BIOS doc/json directory<input class="text-input" bind:value={dcs.cataloguePath} placeholder="C:\Users\…\Saved Games\DCS\Scripts\DCS-BIOS\doc\json" /></label>
      <label class="wide">Aircraft module<input class="text-input" bind:value={draft.aircraftModule} maxlength="63" placeholder="For example A-10C; empty clears the selection" /></label>
      <p class="help wide">Use the exact module name from the installed catalogue. This selects authoring metadata; the running aircraft still needs to match the verified game context.</p>
      <div class="fields"><label>Command host<input class="text-input" bind:value={dcs.commandHost} /></label><label>Command UDP port<input class="text-input" type="number" min="1" max="65535" bind:value={dcs.commandPort} /></label><label>Listen address<input class="text-input" bind:value={dcs.listenAddress} /></label><label>Listen UDP port<input class="text-input" type="number" min="1" max="65535" bind:value={dcs.listenPort} /></label><label>Multicast group<input class="text-input" bind:value={dcs.multicastGroup} placeholder="Empty for unicast" /></label><label>Multicast interface<input class="text-input" bind:value={dcs.multicastInterface} /></label></div>
    {:else if xplane}
      <div class="fields"><label>Simulator host<input class="text-input" bind:value={xplane.host} /></label><label>Simulator UDP port<input class="text-input" type="number" min="1" max="65535" bind:value={xplane.port} /></label><label>Listen UDP port<input class="text-input" type="number" min="0" max="65535" bind:value={xplane.listenPort} /></label><label>Updates per second<input class="text-input" type="number" min="1" max="20" bind:value={xplane.updateHz} /></label></div>
      <p class="help">X-Plane {xplane.version} uses RREF telemetry on a local IPv4 loopback host (127.x.x.x), so Cockpit can verify the producing process. A listen port of 0 lets Cockpit choose an available local port. This provider exposes telemetry only.</p>
    {:else}<p class="help">Native shared-memory telemetry. Cockpit verifies the actual game process and build; no process ID or synthetic source is configured here. This provider exposes telemetry only.</p>{/if}
    <label class="freshness">Telemetry freshness (ms)<input class="text-input" type="number" min="100" max="10000" bind:value={draft.connection.freshnessMs} /></label>
    {#if game.integrationSetup?.catalogueRevision}<p class="help revision">Catalogue revision <code>{game.integrationSetup.catalogueRevision}</code></p>{/if}
    {#if remoteChanged && dirty && !saving}<p class="warning" role="status">These settings changed elsewhere. Refresh the saved settings before saving again; your edits are still here.</p>{/if}
    {#if error}<p class="help warning">{error}</p>{/if}
    {#if notice}<Notification message={notice} />{/if}
    <div class="settings-actions"><button class="text-button" disabled={saving || busy || !dirty && !remoteChanged} onclick={discard}>{remoteChanged ? 'Refresh saved settings' : 'Discard changes'}</button><button class="secondary-button" disabled={saving || busy || !dirty || !!error || remoteChanged} onclick={save}><Icon name="save" size={13} /> {saving ? 'Saving…' : 'Save game settings'}</button></div>
    <p class="help">Save updates provider settings and rechecks affected game branches. Export-script installation and repair use the separate setup controls below.</p>
  {/if}
</section>

<style>
  .provider-settings{display:grid;gap:14px;border-top:1px solid var(--line);margin-top:20px;padding-top:20px;}.settings-heading{display:flex;justify-content:space-between;gap:12px;}h3{font-size:13px;margin:0 0 7px;}p{margin:0;}label{display:grid;gap:7px;font-size:10px;color:var(--muted);}input{min-width:0;width:100%;}.settings-heading p,.help{font-size:10px;color:var(--muted);line-height:1.75;}.settings-state{font-size:9px;color:var(--soft);white-space:nowrap;}.fields{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px;}.freshness{max-width:220px;}.settings-actions{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap;}.warning{font-size:10px;color:var(--warn,#e2b76a);line-height:1.7;}.revision{overflow-wrap:anywhere;}code{font-size:9px;}@media(max-width:650px){.fields{grid-template-columns:1fr;}.settings-heading{display:block;}.settings-state{display:block;margin-top:8px;}}
</style>
