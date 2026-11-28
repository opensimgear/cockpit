import {readFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {compareMacEvidence} from './verify-macos-core.mjs';

export function options(argv) {
  const result={};
  for(let index=0;index<argv.length;index++) {
    const name=argv[index];
    if(!['--app','--probe'].includes(name))throw Error(`Unknown argument ${name}`);
    const value=argv[++index];
    if(!value||value.startsWith('--'))throw Error(`Missing value for ${name}`);
    result[name.slice(2)]=resolve(value);
  }
  if(!result.app||!result.probe)throw Error('Provide --app and --probe JSON receipts.');
  return result;
}

export async function verify(args) {
  const [app,probe]=await Promise.all([
    readFile(args.app,'utf8').then(JSON.parse),readFile(args.probe,'utf8').then(JSON.parse)
  ]);
  return compareMacEvidence(app,probe);
}

if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href) {
  verify(options(process.argv.slice(2))).then(result=>console.log(JSON.stringify(result)))
    .catch(error=>{console.error(error.message);process.exitCode=1;});
}
