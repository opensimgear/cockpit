import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { promisify } from 'node:util';

export async function integrationObservation({ page, modern, ok, writer, mutation, artifacts, until }) {
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
    const nextContext = async predicate => {
      for (;;) {
        const newline = buffer.indexOf('\n');
        if (newline < 0) {
          const { value, done } = await reader.read();
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
        const context = frame.payload?.value?.profileContext;
        if (context && predicate(context)) return context;
      }
    };
    await nextContext(context => context.status === 'not-running');
    child = spawn(executable, [], { cwd: installPath, windowsHide: true, stdio: 'ignore' });
    await once(child, 'spawn');
    const running = await nextContext(context => context.status === 'verified' && context.processSession);
    await until(async () => (await readContext())?.processSession === running.processSession,
      'Native detail missed the same process session');
    const exited = once(child, 'exit');
    child.kill();
    await exited;
    await nextContext(context => context.status === 'not-running' && context.processSession === null);
    await until(async () => (await readContext())?.status === 'not-running', 'Native detail retained the exited process');
    return { gameId, fixtureProcess: true, verifiedAndWithdrawn: true, statusReadsWhileSubscribed: 0 };
  } finally {
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
