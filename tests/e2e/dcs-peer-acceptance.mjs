// Independent catalogue/UDP software peer proof; no retired profile/runtime is started.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createSocket} from 'node:dgram';
import {resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {processClient} from './process-client.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function udpPort(){const socket=createSocket('udp4');await new Promise(resolve=>socket.bind(0,'127.0.0.1',resolve));const port=socket.address().port;await new Promise(resolve=>socket.close(resolve));return port;}
test('radio peer validates named catalogue commands and bounds its diagnostic history', { timeout: 20000 }, async () => {
  const receive = createSocket('udp4'), send = createSocket('udp4');
  await new Promise(resolve => receive.bind(0, '127.0.0.1', resolve));
  const commandPort = await udpPort();
  const datagrams = [];
  receive.on('message', bytes => { datagrams.push(bytes); if (datagrams.length > 64) datagrams.shift(); });
  const peer = processClient(process.execPath, [resolve(root, 'simulation/dcs-bios/peer.mjs'), '--command-port', String(commandPort),
    '--export-port', String(receive.address().port), '--interval-ms', '10'], process.env);
  let sequence = 0;
  const admin = async command => {
    const reply = await peer.line(`sim ${++sequence} ${command}`);
    assert.equal(reply.ok, true, `${command}: ${reply.error}`); return reply.simulation;
  };
  const wait = async accepts => {
    const deadline = Date.now() + 3000;
    let state;
    do { state = await admin('inspect'); if (accepts(state)) return state; await sleep(10); } while (Date.now() < deadline);
    assert.fail(`Peer state did not settle: ${JSON.stringify(state)}`);
  };
  const transmit = lines => new Promise((resolve, reject) => send.send(Buffer.from(lines), commandPort, '127.0.0.1', error => error ? reject(error) : resolve()));
  try {
    await wait(() => datagrams.length > 0);
    await admin('warning 1');
    await transmit('UNKNOWN 1\nUHF_VOL 65536\nUHF_VOL +0\nUHF_SQUELCH INC\nUHF_TEST 2\n');
    await transmit('UHF_PRESET_SEL INC\nUHF_PRESET_SEL DEC\nUHF_PRESET_SEL 20\nUHF_10MHZ_SEL 9\nUHF_VOL 20000\nUHF_VOL +1024\nUHF_VOL -24\nUHF_SQUELCH 1\nUHF_TEST 1\nUHF_TEST 0\nUFC_MASTER_CAUTION 1\nUFC_MASTER_CAUTION 0\n');
    let state = await wait(value => value.commandCount === 12);
    assert.deepEqual([state.warning, state.preset, state.frequency, state.volume, state.squelch, state.testPressed], [false, '20', '290.000', 21000, 1, 0]);
    assert.ok(state.commands.every(command => command.receivedAtMs > 0 && command.sequence > 0));
    for (let chunk = 0; chunk < 6; chunk++) await transmit('UHF_TEST 0\n'.repeat(50));
    state = await wait(value => value.commandCount === 312);
    assert.equal(state.commands.length, 256);
    assert.equal(state.commands[0].sequence, 57);
    assert.equal(state.commands.at(-1).sequence, 312);
    await admin('frequency 123.450'); await admin('preset 20'); await admin('volume 0');
    await admin('pause 1'); await admin('half_frame 1'); await admin('pause 0');
    state = await wait(value => value.holdingFrame);
    await admin('pause 1'); await admin('complete'); await admin('half_frame 0');
    await admin('fragment 1'); await admin('freeze 1'); await admin('pause 0');
    const frozen = await admin('inspect'); await sleep(30);
    assert.equal((await admin('inspect')).counter, frozen.counter);
    await admin('freeze 0'); await admin('flood 1'); await admin('rate 200');
    await wait(value => value.frequency !== '123.450');
    await admin('flood 0'); await admin('malformed 1'); await admin('malformed 0'); await admin('malformed');
    await admin('frequency_hex ff'); await admin('aircraft NONE');
    assert.equal((await admin('inspect')).aircraft, '');
    await admin('aircraft A-10C_2'); await admin('coalesce 4');
    await wait(() => datagrams.some(bytes => bytes.length > 200));
    const invalid = await peer.line(`sim ${++sequence} rate 1001`);
    assert.equal(invalid.ok, false);
  } finally {
    await peer.stop();
    await Promise.all([receive, send].map(socket => new Promise(resolve => socket.close(resolve))));
  }
});
