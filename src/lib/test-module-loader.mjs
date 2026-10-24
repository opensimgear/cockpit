import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import {webcrypto} from 'node:crypto';

/** Load pure TS helpers with their real dependencies and source filenames so
 * Node's built-in coverage reports include the exercised production code. */
export async function loadModules(names) {
  const exportsByName = new Map();
  async function load(name) {
    name=name.replace(/\.ts$/,'');
    if(exportsByName.has(name))return;
    const filename = fileURLToPath(new URL(`${name}.ts`, import.meta.url));
    const exports = {};
    const code = ts.transpileModule(await readFile(filename, 'utf8'), { fileName: filename, compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, inlineSourceMap: true, inlineSources: true } }).outputText;
    // New pure helpers may add dependencies without every consumer fixture
    // needing to repeat their transitive import order.
    for(const match of code.matchAll(/require\(["'](\.\/[^"']+)["']\)/g))await load(match[1]);
    vm.runInNewContext(code, { exports, TextEncoder, structuredClone, crypto:webcrypto, require(name) {
      const key=name.replace(/\.ts$/,'');
      if (!exportsByName.has(key)) throw new Error(`Load dependency ${name} before its consumer.`);
      return exportsByName.get(key);
    } }, { filename });
    exportsByName.set(name, exports);
  }
  for (const name of names) await load(name);
  return exportsByName;
}
