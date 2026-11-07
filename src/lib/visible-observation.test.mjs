import assert from 'node:assert/strict';
import test from 'node:test';
import {visibleObservation} from './visible-observation.ts';

test('visible demand starts once and retires on hide or close', () => {
  const document = new EventTarget();
  document.hidden = false;
  globalThis.document = document;
  const events = [];
  const topic = 'system.inventory.vjoy';
  const observations = {subscribe(actual) {
    assert.equal(actual, topic);
    events.push('open');
    return () => events.push('close');
  }};
  const demand = visibleObservation(observations, topic, () => {}, () => events.push('retire'));
  const hide = hidden => {
    document.hidden = hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  };
  try {
    hide(true);
    demand.setVisible(true);
    assert.deepEqual(events, []);
    hide(false);
    demand.setVisible(true);
    assert.deepEqual(events, ['open']);
    hide(true);
    assert.deepEqual(events, ['open', 'close', 'retire']);
    demand.setVisible(false);
    hide(false);
    assert.deepEqual(events, ['open', 'close', 'retire']);
    demand.setVisible(true);
    demand.setVisible(false);
    demand.setVisible(true);
    demand.close();
    demand.close();
    demand.setVisible(true);
    hide(false);
    assert.deepEqual(events, ['open', 'close', 'retire', 'open', 'close', 'retire', 'open', 'close', 'retire']);
  } finally {
    demand.close();
    delete globalThis.document;
  }
});
