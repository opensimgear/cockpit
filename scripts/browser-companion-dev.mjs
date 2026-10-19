import { build } from 'vite';
import { utimes } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

/** Keep the embedded browser client in sync with shared UI.
 * @returns {import('vite').Plugin}
 */
export function browserCompanionDev() {
  return {
    name: 'browser-companion-dev', apply: 'serve',
    async configureServer(server) {
      const watcher=await build({configFile:fileURLToPath(new URL('../apps/browser-companion/vite.config.ts',import.meta.url)),build:{watch:{}},logLevel:'warn'});
      if(!('on' in watcher))throw new Error('Browser companion build did not start its watcher.');
      // Tauri ignores dist through .gitignore, even in additional watch folders.
      // Notify its tracked build script only after the entire bundle is ready.
      watcher.on('event',async event=>{
        if(event.code==='END' && process.env.TAURI_ENV_PLATFORM){
          const now=new Date();await utimes(fileURLToPath(new URL('../src-tauri/build.rs',import.meta.url)),now,now);
        }
      });
      const close=()=>watcher.close();
      server.httpServer?.once('close',close);
      await new Promise((resolve,reject)=>{
        /** @param {{code:string,error?:unknown}} event */
        const ready=event=>{
          if(event.code==='END'){watcher.off('event',ready);resolve(undefined);}
          else if(event.code==='ERROR'){watcher.off('event',ready);void close();reject(event.error);}
        };
        watcher.on('event',ready);
      });
    },
  };
}
