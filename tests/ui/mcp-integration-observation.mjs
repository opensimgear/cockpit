import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { promisify } from 'node:util';
import { GamePages } from '../../src/lib/game-read-models/pages.ts';

export async function integrationObservation({ page, modern, ok, writer, readClient, mutation, artifacts, until }) {
  const gameId = 'dcs';
  const uri = `cockpit://observations/games.detail.${gameId}`;
  const games = (await page.native({ command: 'game_get_state' })).games;
  assert.ok(!games.catalogue.some(game => game.id === gameId), 'Use a dev Cockpit without a registered DCS installation');
  const initialMode = (await page.native({ command: 'profile_list' })).profiles.session.selectionMode;
  const installPath = resolve(artifacts, 'dcs-fixture');
  const executable = resolve(installPath, 'bin/DCS.exe');
  const source = resolve(installPath, 'DcsFixture.cs');
  await mkdir(resolve(installPath, 'bin'), { recursive: true });
  await writeFile(source, 'class DcsFixture { static void Main() { System.Threading.Thread.Sleep(System.Threading.Timeout.Infinite); } }');
  const compiler = resolve(process.env.WINDIR, 'Microsoft.NET/Framework64/v4.0.30319/csc.exe');
  await promisify(execFile)(compiler, ['/nologo', '/target:winexe', `/out:${executable}`, source], { windowsHide: true });
  let registered, child, reader;
  const resolution = new AbortController();
  let pageReads = 0;
  const pages = new GamePages((id, offset) => {
    pageReads++;
    return ok(readClient, 'game_read_model_page', { id, offset });
  });
  try {
    await page.native({ command: 'profile_selection_mode', mode: 'manual' });
    const result = await ok(writer, 'game_add', mutation({ name: 'DCS fixture', installPath, executable }));
    registered = result.gameId;
    assert.equal(registered, gameId);
    await page.evaluate(async id => {
      const { observeGameDetails } = await import('/src/lib/game-read-models/shared.ts');
      window.__integrationStop = observeGameDetails(id, state => { window.__integrationState = state; });
    }, gameId);
    const readContext = () => page.evaluate(() => window.__integrationState?.value?.value?.profileContext);
    await until(async () => (await readContext())?.status === 'not-running', 'Native integration baseline did not settle');
    const response = await modern('subscriptions/listen', { notifications: { resourceSubscriptions: [uri] } });
    assert.equal(response.status, 200);
    assert.match(response.headers.get('content-type'), /text\/event-stream/);
    reader = response.body.getReader();
    let buffer = '';
    const decoder = new TextDecoder();
    const nextContext = async (label, predicate) => {
      const seen = [];
      for (;;) {
        const newline = buffer.indexOf('\n');
        if (newline < 0) {
          const { value, done } = await reader.read().catch(async error => {
            const portal = await readContext().catch(() => null);
            throw Error(`Integration stream did not reach ${label}; portal status ${portal?.status}, MCP states ${seen.join(', ')}`, { cause: error });
          });
          assert.equal(done, false, 'Integration stream closed before the expected context');
          buffer += decoder.decode(value, { stream: true });
          continue;
        }
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        if (!line.startsWith('data:')) continue;
        const event = JSON.parse(line.slice(5));
        assert.ok(!event.error, JSON.stringify(event.error));
        if (event.params?.uri !== uri) continue;
        const frame = event.params._meta['opensimgear.com/observation'];
        assert.notEqual(frame.kind, 'loss');
        const detail = await pages.resolve(frame.payload?.value, resolution.signal);
        const context = detail?.profileContext;
        if (context && seen.at(-1) !== context.status) {
          if (seen.length === 12) seen.shift();
          seen.push(context.status);
        }
        if (context && predicate(context)) return context;
      }
    };
    await nextContext('initial idle', context => context.status === 'not-running');
    child = spawn(executable, [], { cwd: installPath, windowsHide: true, stdio: 'ignore' });
    await once(child, 'spawn');
    const running = await nextContext('verified process', context => context.status === 'verified' && context.processSession);
    await until(async () => (await readContext())?.processSession === running.processSession,
      'Native detail missed the same process session');
    const exited = once(child, 'exit');
    child.kill();
    await exited;
    await nextContext('process exit', context => context.status === 'not-running' && context.processSession === null);
    await until(async () => (await readContext())?.status === 'not-running', 'Native detail retained the exited process');
    return { gameId, fixtureProcess: true, verifiedAndWithdrawn: true, statusReadsWhileSubscribed: 0, immutablePageReads: pageReads };
  } finally {
    resolution.abort();
    try {
      if (child && child.exitCode === null && child.signalCode === null && child.pid) {
        const exited = once(child, 'exit');
        child.kill();
        await exited;
      }
      await reader?.cancel().catch(() => {});
      await page.evaluate(() => {
        window.__integrationStop?.();
        delete window.__integrationStop;
        delete window.__integrationState;
      });
      if (registered) await page.native({ command: 'game_remove', gameId: registered });
    } finally {
      await page.native({ command: 'profile_selection_mode', mode: initialMode });
    }
  }
}
