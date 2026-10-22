import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';

const { createCanvasNavigationStore, canvasNavigation, canvasWheelNavigation, CANVAS_NAVIGATION_STORAGE_KEY } = (await loadModules(['./canvas-navigation'])).get('./canvas-navigation');
const plain = value => JSON.parse(JSON.stringify(value));
function current(store) { let value; store.subscribe(next => { value = next; })(); return plain(value); }
function memory(saved = null) {
  const writes = [];
  return { writes, getItem(key) { assert.equal(key, CANVAS_NAVIGATION_STORAGE_KEY); return saved; }, setItem(key, value) { assert.equal(key, CANVAS_NAVIGATION_STORAGE_KEY); saved = value; writes.push(JSON.parse(value)); } };
}

test('canvas navigation defaults to pan and works without browser storage', () => {
  assert.deepEqual(current(canvasNavigation), { wheelAction: 'pan', storageError: null });
  const store = createCanvasNavigationStore(null); store.setWheelAction('zoom');
  assert.deepEqual(current(store), { wheelAction: 'zoom', storageError: null });
});

test('only known stored wheel actions are restored and unknown values use pan', () => {
  assert.deepEqual(current(createCanvasNavigationStore(memory('{"wheelAction":"zoom"}'))), { wheelAction: 'zoom', storageError: null });
  for (const saved of [null, 'null', '[]', 'true', '42', '{}', '{"wheelAction":"pan"}', '{"wheelAction":"scroll"}', '{"wheelAction":true}']) {
    assert.deepEqual(current(createCanvasNavigationStore(memory(saved))), { wheelAction: 'pan', storageError: null });
  }
});

test('wheel changes persist only app navigation preferences and survive recreation', () => {
  const storage = memory(), store = createCanvasNavigationStore(storage);
  store.setWheelAction('zoom'); assert.equal(current(createCanvasNavigationStore(storage)).wheelAction, 'zoom');
  store.setWheelAction('pan');
  assert.deepEqual(storage.writes, [{ wheelAction: 'zoom' }, { wheelAction: 'pan' }]);
  assert.equal(CANVAS_NAVIGATION_STORAGE_KEY, 'osg-canvas-navigation');
});

test('unchanged and invalid wheel changes neither save nor notify subscribers', () => {
  const storage = memory(), store = createCanvasNavigationStore(storage), seen = [];
  store.subscribe(value => seen.push(plain(value)));
  store.setWheelAction('pan'); for (const value of ['scroll', '', null, 1]) store.setWheelAction(value);
  assert.equal(storage.writes.length, 0); assert.equal(seen.length, 1);
});

test('unreadable storage reports a scoped failure while navigation remains usable', () => {
  assert.match(current(createCanvasNavigationStore(memory('{'))).storageError, /could not be loaded/);
  const store = createCanvasNavigationStore({ getItem() { throw Error('unavailable'); }, setItem() { throw Error('unavailable'); } });
  assert.match(current(store).storageError, /could not be loaded/);
  store.setWheelAction('zoom');
  assert.equal(current(store).wheelAction, 'zoom'); assert.match(current(store).storageError, /could not be saved/);
});

test('failed saves keep session choice and retrying the same action clears a recovered failure', () => {
  let blocked = true;
  const storage = memory(), store = createCanvasNavigationStore({ getItem: storage.getItem, setItem(...args) { if (blocked) throw Error('full'); storage.setItem(...args); } });
  store.setWheelAction('zoom'); assert.equal(current(store).wheelAction, 'zoom'); assert.match(current(store).storageError, /could not be saved/);
  blocked = false; store.setWheelAction('zoom');
  assert.deepEqual(current(store), { wheelAction: 'zoom', storageError: null }); assert.deepEqual(storage.writes, [{ wheelAction: 'zoom' }]);
});

test('subscribers receive immutable snapshots and can unsubscribe', () => {
  const store = createCanvasNavigationStore(null), seen = [];
  const unsubscribe = store.subscribe(value => { assert.equal(Object.isFrozen(value), true); seen.push(plain(value)); });
  store.setWheelAction('zoom'); unsubscribe(); store.setWheelAction('pan');
  assert.deepEqual(seen, [{ wheelAction: 'pan', storageError: null }, { wheelAction: 'zoom', storageError: null }]);
});

test('shared canvas wheel actions reverse with either modifier and normalize pixel, line and page scrolling', () => {
  for (const action of ['pan','zoom']) for (const modifier of [{},{ctrlKey:true},{metaKey:true},{ctrlKey:true,metaKey:true}]) {
    for (const [deltaMode,unit] of [[0,1],[1,16],[2,500]]) {
      const event={ctrlKey:false,metaKey:false,deltaX:2,deltaY:-3,deltaMode,...modifier};
      const navigation=canvasWheelNavigation(action,event,500);
      assert.equal(navigation.zoom,(action==='zoom')!==!!(event.ctrlKey||event.metaKey));
      assert.equal(navigation.deltaX,2*unit);assert.equal(navigation.deltaY,-3*unit);
      assert.equal(navigation.multiplier,1.1);
    }
  }
  assert.equal(canvasWheelNavigation('zoom',{deltaX:0,deltaY:1},500).multiplier,1/1.1);
  assert.equal(canvasWheelNavigation('zoom',{deltaX:5,deltaY:0},500).multiplier,1);
});
