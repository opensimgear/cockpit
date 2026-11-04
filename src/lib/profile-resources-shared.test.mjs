import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

test('resource views share one reader and release it when the final view unmounts', async () => {
  const url = new URL('./profile-resources-shared.ts', import.meta.url);
  const source = await readFile(url, 'utf8');
  const { outputText } = ts.transpileModule(source, {
    fileName: fileURLToPath(url),
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, inlineSourceMap: true, inlineSources: true },
  });
  const exports = {};
  const readers = [];
  const visibility = new Set(), topics = [];
  let nativeResult = { profileResources: { revision: '1' } };
  const dependencies = {
    './profile-inventory': { ProfileInventoryClient: class { constructor(send) { this.send = send; } } },
    './gateway': { gatewayRequest: async () => nativeResult },
    './streams/shared': { SharedObservations: class { subscribe(topic) { topics.push(topic); return () => {}; } } },
    './profile-resource-observation': {
      followProfileResources(subscribe, client, receive, visible) {
        subscribe(receive);
        const reader = { receive, client, visible, disposed: false, refreshVisible() {}, dispose() { reader.disposed = true; } };
        readers.push(reader);
        return reader;
      },
    },
  };
  vm.runInNewContext(outputText, {
    exports,
    require: name => dependencies[name],
    document: {
      addEventListener: (_, listener) => visibility.add(listener),
      removeEventListener: (_, listener) => visibility.delete(listener),
    },
  }, { filename: fileURLToPath(url) });
  const observe = exports.observeProfileResources;
  const first = [], second = [], third = [];
  const stopFirst = observe(page => first.push(page));
  assert.equal(readers[0].visible(),true);
  assert.deepEqual(topics,['profiles.resources']);
  assert.equal((await readers[0].client.send({})).revision,'1');
  nativeResult = {};
  await assert.rejects(readers[0].client.send({}),/Resource inventory is unavailable/);
  const page = { revision: '1' };
  readers[0].receive(page);
  const stopSecond = observe(value => second.push(value));
  assert.equal(readers.length, 1);
  assert.deepEqual(first, [page]);
  assert.deepEqual(second, [page]);
  stopFirst();
  readers[0].receive(null, 'stale');
  assert.deepEqual(first, [page]);
  assert.deepEqual(second, [page, null]);
  assert.equal(readers[0].disposed, false);
  stopSecond();
  assert.equal(readers[0].disposed, true);
  assert.equal(visibility.size, 0);
  const stopThird = observe(value => third.push(value));
  assert.equal(readers.length, 2);
  assert.deepEqual(third, []);
  stopThird();
});
