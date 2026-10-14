import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';

/** Load pure TS helpers with their real dependencies and source filenames so
 * Node's built-in coverage reports include the exercised production code. */
export async function loadModules(names) {
  const exportsByName = new Map();
  for (const name of names) {
    const filename = new URL(`${name}.ts`, import.meta.url).pathname;
    const exports = {};
    const code = ts.transpileModule(await readFile(filename, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    vm.runInNewContext(code, { exports, TextEncoder, require(name) {
      if (!exportsByName.has(name)) throw new Error(`Load dependency ${name} before its consumer.`);
      return exportsByName.get(name);
    } }, { filename });
    exportsByName.set(name, exports);
  }
  return exportsByName;
}
