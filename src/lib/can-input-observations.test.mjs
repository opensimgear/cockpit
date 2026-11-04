import assert from 'node:assert/strict';
import test from 'node:test';
import { canInputTopic, canPanelPreview } from './can-input-observations.ts';

const gateway = 'stm32:00112233445566778899aabb';
const moduleId = '00112233445566778899aabbccddeeff';
const topic = `can.inputs.${gateway.slice(6)}.${moduleId}`;

test('CAN observation topics bind an exact physical gateway and module', () => {
  assert.equal(canInputTopic(gateway, moduleId), topic);
  assert.equal(canInputTopic(gateway.replace('stm32:', 'pico:'), moduleId), undefined);
  assert.equal(canInputTopic(gateway, moduleId.toUpperCase()), undefined);
  assert.equal(canInputTopic(gateway, `${moduleId}.other`), undefined);
});

test('CAN preview keeps raw axes and withdraws them on loss or wrong scope', () => {
  const payload = {
    status: 'ready', route: { epoch: 1, node: 32, sourceNode: 127, boot: 3, lease: 4, configurationGeneration: 5 },
    held: [true, false], axes: [0, 65535, 7, 0, 0, 0, 0, 0],
    axisAvailable: [true, true, false, false, false, false, false, false],
  };
  const state = { status: 'ready', frame: { topic, source: 'can-inputs', payload } };
  const preview = canPanelPreview(state, topic);
  assert.deepEqual(preview.axes.slice(0, 3), [0, 65535, 7]);
  assert.equal(preview.axisAvailable[2], false);
  assert.equal(canPanelPreview({ ...state, status: 'lost' }, topic), undefined);
  assert.equal(canPanelPreview(state, `${topic}0`), undefined);
  assert.equal(canPanelPreview({ ...state, frame: { ...state.frame, payload: { status: 'unavailable' } } }, topic), undefined);
  payload.axes[2] = 65536;
  assert.equal(canPanelPreview(state, topic), undefined);
});
