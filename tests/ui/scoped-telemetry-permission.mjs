import assert from 'node:assert/strict';
import {until} from '../../scripts/reactive/cdp.mjs';

export async function verifyScopedTelemetryDenial({page, request, gameId}) {
  const preference = (await request({command:'ui_preferences_get', key:'game-process-events'})).uiPreferences;
  assert.equal(preference.value, false, 'Use a private Cockpit with Windows discovery events off');
  const boundary = await until(async () => {
    const {games} = await request({command:'game_get_state'});
    const game = games.catalogue.find(row => row.id === gameId);
    return game?.processMonitor?.state === 'needs-permission'
      && games.detectedGames.some(row => row.gameId === gameId)
      && game;
  }, Boolean, 30_000);
  assert.match(boundary.processMonitor.details, /0x80041003|permission denied|access is denied/i);

  const {gameTelemetry} = await request({command:'game_telemetry_inspect', gameId, search:'engine.rpm', offset:0, limit:32});
  const telemetry = gameTelemetry.games.find(row => row.id === gameId);
  assert.ok(telemetry, 'The inspector must describe the blocked game');
  assert.notEqual(telemetry.status, 'live');
  assert.ok(telemetry.channels.every(channel => channel.status !== 'known' && channel.value === null));

  await until(() => page.evaluate(`(() => {
    const games = document.querySelector('nav[aria-label="Main navigation"] button[title="Games"]');
    if (!games) return false;
    if (!games.classList.contains('current')) { games.click(); return false; }
    const view = document.querySelector('.games-workspace');
    if (view?.querySelector('[aria-label="Game name"]')?.value === ${JSON.stringify(boundary.name)}) return true;
    const tile = view?.querySelector(${JSON.stringify(`[data-game-id="${gameId}"] .tile-details`)});
    if (tile) tile.click();
    else view?.querySelector('button.header-back')?.click();
    return false;
  })()`));
  const readNotice = () => page.evaluate(`(() => {
    const section = document.querySelector('[aria-label="Process monitoring"]');
    return {hash:location.hash, heading:document.querySelector('main h1')?.textContent,
      selected:document.querySelector('[aria-label="Game name"]')?.value,
      status:section?.querySelector('[role="status"]')?.textContent,
      button:section?.querySelector('button')?.textContent.trim(),
      text:section?.querySelector('.monitor-help')?.textContent.trim()};
  })()`);
  const notice = await until(readNotice, value => value.button === 'Allow'
    && value.text?.includes('competing simulator instances') && value.text.includes('one-second fallback'))
    .catch(async () => { throw Error(`${gameId} did not show the scoped Allow action: ${JSON.stringify(await readNotice())}`); });
  return {boundary:boundary.processMonitor, notice, telemetryStatus:telemetry.status};
}
