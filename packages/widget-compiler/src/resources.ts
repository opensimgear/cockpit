import {assertSource} from './source.ts';
/** Static URLs become bundled assets in Cockpit and exact file bytes in Node. */
export async function loadCompilerResource(url:URL):Promise<Uint8Array>{
  if(url.protocol==='file:'){const fs=await import('node:fs/promises');return new Uint8Array(await fs.readFile(url));}
  const response=await fetch(url);assertSource(response.ok,'Compiler resource could not be loaded');return new Uint8Array(await response.arrayBuffer());
}
