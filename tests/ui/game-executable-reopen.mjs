import assert from 'node:assert/strict';
import {copyFile, mkdir, mkdtemp} from 'node:fs/promises';
import {resolve} from 'node:path';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_GAME_EXECUTABLE_BROWSER_URL;
const enabled = process.platform === 'win32' && (browserUrl || process.env.OSG_GAME_EXECUTABLE_NATIVE === '1');
const field = 'document.querySelector(\'[aria-label="Game executable"]\')';

test('the game executable follows external edits and preserves a local draft', {
  skip: !enabled, timeout: 120_000,
}, async () => {
  const root = resolve('.research/game-executable-reopen');
  await mkdir(root, {recursive:true});
  const installation = await mkdtemp(resolve(root, 'installation-'));
  const original = resolve(installation, 'original.exe');
  const replacement = resolve(installation, 'replacement.exe');
  const third = resolve(installation, 'third.exe');
  for (const path of [original, replacement, third]) {
    await copyFile(resolve(process.env.SystemRoot, 'System32/where.exe'), path);
  }
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(
    `import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`,
  );
  let gameId;
  const executableWrites = [];
  const stopTrace = page.listen(({method, params}) => {
    if (method !== 'Network.requestWillBeSent' || !params.request.postData) return;
    const {pathname} = new URL(params.request.url);
    if (pathname !== '/gateway_request' && pathname !== '/api/gateway/') return;
    const body = JSON.parse(params.request.postData);
    const command = body.request ?? body;
    if (command.gameId === gameId && command.command === 'game_save' && 'executable' in command) {
      executableWrites.push(command.executable);
    }
  });
  try {
    await until(() => page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    const added = await request({command:'game_add', name:`Executable reopen ${Date.now()}`,
      installPath:installation, executable:original});
    gameId = added.games.addedGameId;
    assert.ok(gameId);
    const installationId = added.games.catalogue.find(game => game.id === gameId).selectedInstallationId;
    assert.ok(installationId);
    const changeExecutable = (executable, name) => request({command:'game_save', gameId, installationId, executable, name});
    const open = () => page.evaluate(`window.dispatchEvent(new CustomEvent('osg:open-game-settings', {
      detail:{gameId:${JSON.stringify(gameId)}}
    }))`);
    const readField = () => page.evaluate(`(() => {
      const input = ${field};
      return input && !input.disabled ? input.value : null;
    })()`);
    await open();
    await until(readField, value => value === 'original.exe');
    await page.call('Network.enable', {maxPostDataSize:65536});

    await changeExecutable(replacement);
    await until(readField, value => value === 'replacement.exe', 10_000);
    await page.evaluate(`(() => { const input = ${field}; input.focus(); input.blur(); })()`);
    await page.evaluate(`(() => {
      const input = ${field};
      input.focus();
      input.value = 'local-draft.exe';
      input.dispatchEvent(new Event('input', {bubbles:true}));
    })()`);
    const changedName = `External edit ${Date.now()}`;
    await changeExecutable(third, changedName);
    await until(() => page.evaluate(`!${field}?.disabled &&
      document.querySelector('[aria-label="Game name"]')?.value === ${JSON.stringify(changedName)}`));
    assert.equal(await readField(), 'local-draft.exe');
    await page.evaluate(`(() => {
      const input = ${field};
      input.focus();
      input.dispatchEvent(new KeyboardEvent('keydown', {key:'Escape', bubbles:true}));
    })()`);
    await until(readField, value => value === 'third.exe');

    await page.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Overview"]').click()`);
    await until(() => page.evaluate(`${field} === null`));
    await changeExecutable(original);
    await open();
    await until(readField, value => value === 'original.exe');
    assert.deepEqual(executableWrites, [replacement, third, original]);
    console.log(JSON.stringify({transport:browserUrl ? 'browser-dev' : 'native', cleanUpdate:true,
      retainedDraft:true, escapeToCurrent:true, reopenedCurrent:true, implicitWrites:0}));
  } finally {
    stopTrace();
    try {
      if (gameId) await request({command:'game_remove', gameId});
    } finally {
      await page.close();
    }
  }
});
