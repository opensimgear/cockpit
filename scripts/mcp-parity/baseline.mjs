import { execFileSync } from 'node:child_process';
import { discover } from './actions.mjs';

export const revision = '3f4dbe696efbd00129f0b4acef3d5fddce5af47f';
export function originalFiles(root, run = execFileSync) {
  const options = {cwd:root, maxBuffer:64 * 1024 * 1024, stdio:['pipe','pipe','pipe']};
  try {
    const paths = run('git', ['ls-tree','-r','-z','--name-only',revision], options).toString().split('\0').filter(path =>
      /^(?:src|apps\/[^/]+\/src|packages\/[^/]+\/src)\/.*\.svelte$/.test(path) ||
      /^src\/lib\/.*\.ts$/.test(path) || /^src-tauri\/src\/.*\.rs$/.test(path));
    const blobs = run('git', ['cat-file','--batch'], {...options, input:paths.map(path => `${revision}:${path}\n`).join('')});
    const files = new Map();
    let offset = 0;
    for (const path of paths) {
      const end = blobs.indexOf(10, offset);
      const header = blobs.subarray(offset,end).toString();
      const match = /^\w+ blob (\d+)$/.exec(header);
      if (!match) throw Error(`Invalid original blob ${path}: ${header}`);
      const length = Number(match[1]);
      offset = end + 1;
      if (offset + length >= blobs.length) throw Error(`Truncated original blob ${path}`);
      files.set(path, blobs.subarray(offset,offset+length).toString('utf8'));
      offset += length + 1;
    }
    return files;
  } catch (error) {
    throw Error(`Cannot verify original parity sources at ${revision}; fetch full git history. ${error.message}`);
  }
}
export function originActions(root, run) {
  return discover(originalFiles(root, run));
}
