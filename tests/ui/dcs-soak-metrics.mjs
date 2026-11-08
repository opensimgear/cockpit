import assert from 'node:assert/strict';
import {alignClock, summarize} from './dcs-panel-presentation-timing.mjs';

export async function traceSoakRequests(cdp, fixtureClientId) {
  await cdp.call('Network.enable', {maxPostDataSize: 65536});
  const counts = {fixture: {}, app: {}, appByView: {}};
  let view;
  const increment = (bucket, command) => {
    bucket[command] = (bucket[command] ?? 0) + 1;
  };
  const stop = cdp.listen(({method, params}) => {
    if (method !== 'Network.requestWillBeSent') return;
    const {request} = params;
    if (request.method !== 'POST' || new URL(request.url).pathname !== '/gateway_request') return;
    let payload;
    try { payload = JSON.parse(request.postData).request; } catch {}
    const command = payload?.command ?? 'unparsed';
    if (payload?.clientId === fixtureClientId) {
      increment(counts.fixture, command);
      return;
    }
    increment(counts.app, command);
    increment(counts.appByView[view ?? 'before-navigation'] ??= {}, command);
  });
  return {counts, setView: next => { view = next; }, stop};
}

export async function traceSoakPresentation(panels) {
  const align = () => Promise.all(panels.map(({page}) => alignClock(page)));
  const initial = await align();
  const samples = [];
  return {
    partial: () => ({initial, samples}),
    record(submission, value, rows) {
      assert.ok(!samples.length || submission.packetSequence > samples.at(-1).packetSequence,
        'Each lamp transition must use a newer ownship packet');
      const paintAtMs = rows.map(({latestPaintTransition}) => {
        assert.equal(latestPaintTransition?.lit, value, 'Missing correlated lamp transition');
        return latestPaintTransition.at;
      });
      samples.push({...submission, value, paintAtMs});
    },
    async finish() {
      const final = await align();
      const panels = initial.map((first, index) => {
        const low = Math.max(first.low, final[index].low);
        const high = Math.min(first.high, final[index].high);
        assert.ok(high >= low, 'Node and browser clock alignment changed during the soak');
        const upperMs = samples.map(row => row.paintAtMs[index] - row.submittedMs - low);
        assert.ok(upperMs.every(ms => ms >= 0), 'A canvas transition predates its UDP submission');
        return {panel: index + 1, initial: first, final: final[index], uncertaintyMs: (high - low) / 2,
          udpToCanvasUpperMs: summarize(upperMs)};
      });
      return {panels, samples};
    },
  };
}
