<script lang="ts">
  import Icon from './Icon.svelte';
  import { gameIntegrationLabel, gameIntegrationLiveLabel, supportedGames, type Game, type GameId } from './games';
  import { gameSettingsHref } from './game-settings-navigation';
  let { setups = [], openGameSettings }: { setups?: Game[]; openGameSettings: (gameId: GameId) => void } = $props();
  const games = $derived(supportedGames.map(game => setups.find(item => item.id === game.id) ?? game));
  function open(event: MouseEvent, gameId: GameId) {
    if (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    event.preventDefault(); openGameSettings(gameId);
  }
</script>

<section class="integration-resources" aria-label="Game integrations">
  <div class="integration-intro"><Icon name="network" size={22} /><div><h2>Game integration resources</h2><p>Choose commands, telemetry and their processing in a profile graph. Each game owns its provider settings.</p></div></div>
  <div class="provider-list">
    {#each games as game (game.id)}
      <article class="provider-card">
        <div class="provider-heading"><span class="provider-icon"><Icon name={game.provider === 'assetto-corsa-rally' ? 'wheel' : 'command'} size={22} /></span><div><h3>{game.name}</h3><p>{game.provider === 'dcs-bios' ? 'DCS-BIOS · commands and telemetry' : game.provider === 'x-plane' ? 'X-Plane RREF · telemetry' : 'Native shared memory · telemetry'}</p></div></div>
        <dl><div><dt>Provider setup</dt><dd>{gameIntegrationLabel(game)}</dd></div><div><dt>Telemetry</dt><dd>{gameIntegrationLiveLabel(game)}</dd></div></dl>
        <p class="provider-note">{game.id === 'dcs' ? 'Saved Games, export setup, aircraft catalogue and connection settings are in DCS game details.' : 'Installation, connection settings and readiness are in this game’s details.'}</p>
        <a class="settings-link" href={gameSettingsHref(game.id)} onclick={event => open(event, game.id)}>Open {game.name} settings <Icon name="chevron" size={13} /></a>
      </article>
    {/each}
  </div>
  <p class="integration-help">Resources remain available to author while a game is closed. Integration branches wait for the active profile and a matching verified game session.</p>
</section>

<style>
  .integration-resources{padding:24px;}.integration-intro{display:flex;gap:14px;align-items:flex-start;margin-bottom:24px;}h2{font-size:17px;margin:0 0 7px;}h3{font-size:14px;margin:0 0 6px;}p{font-size:11px;color:var(--muted);line-height:1.75;margin:0;}.provider-list{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:16px;}.provider-card{min-width:0;display:flex;flex-direction:column;border:1px solid var(--line);border-radius:8px;padding:20px;gap:18px;}.provider-heading{display:flex;align-items:center;gap:12px;}.provider-icon{display:grid;place-items:center;width:42px;height:42px;border:1px solid var(--line);border-radius:9px;color:var(--accent);flex-shrink:0;}dl{display:grid;gap:10px;margin:0;}dl>div{display:grid;grid-template-columns:110px 1fr;gap:10px;font-size:11px;}dt{color:var(--muted);}dd{margin:0;color:var(--soft);}.provider-note{flex:1;}.settings-link{display:flex;align-items:center;gap:7px;color:var(--accent);font-size:11px;text-decoration:none;align-self:flex-start;}.settings-link:hover{text-decoration:underline;}.settings-link:focus-visible{outline:2px solid var(--accent);outline-offset:5px;}.integration-help{margin-top:22px;}@media(max-width:850px){.provider-list{grid-template-columns:1fr;}}@media(max-width:600px){.integration-resources{padding:16px;}dl>div{grid-template-columns:1fr;gap:4px;}}
</style>
