import { initWasm, Resvg } from '@resvg/resvg-wasm';
import { widgetDigest } from '../../widget-runtime/src/artifact.ts';
import { loadCompilerResource } from './resources.ts';
import { assertSource } from './source.ts';

const fonts = [
  { url: new URL('../../companion-ui/assets/fonts/osg-sans.ttf', import.meta.url), hash: '94bbd25a18ca665999feb05a537de9fd2b860dcfb78bbe9ca00270825bf235da' },
  { url: new URL('../../companion-ui/assets/fonts/osg-mono.ttf', import.meta.url), hash: '6a3412f058c7d8dfd9170c41e85ade48e5156ecb89356110ca57a0a27734af46' },
];
const wasmUrl = new URL('../node_modules/@resvg/resvg-wasm/index_bg.wasm', import.meta.url);
// Vite can replace this module without replacing the underlying resvg module.
// Share initialization by resvg function identity within this JavaScript realm.
const initializationKey = Symbol.for('opensimgear.widget-compiler.resvg');
const realm = globalThis as typeof globalThis & {
  [initializationKey]?: WeakMap<typeof initWasm, Promise<void>>;
};
const initializations = realm[initializationKey] ??= new WeakMap();
export const compilerFontHashes = Object.freeze(fonts.map(font => font.hash));
let ready: Promise<void> | undefined;
let fontBuffers: Uint8Array[] = [];

/** One resource owner serves all compiler module instances in the same runtime. */
export async function initializeWidgetCompiler(resources?: { fonts?: Uint8Array[] }): Promise<void> {
  assertSource(!resources || Object.keys(resources).every(key => key === 'fonts'), 'Compiler resource overrides only accept the verified font bank');
  if (!ready) ready = (async () => {
    const bytes = resources?.fonts ?? await Promise.all(fonts.map(font => loadCompilerResource(font.url)));
    assertSource(bytes.length === fonts.length, 'Compiler needs its pinned font bank');
    for (let index = 0; index < bytes.length; index++) {
      assertSource(await widgetDigest(bytes[index]) === fonts[index].hash, 'Compiler font hash differs; update compiler identity before changing fonts');
    }
    const wasm = await loadCompilerResource(wasmUrl);
    assertSource(await widgetDigest(wasm) === '22bf6e9f9a100d972da0411a69c5ba504367fc1fa87b3b64e3f35e53926d2d70', 'Compiler rasterizer hash differs; update compiler identity before changing WASM');
    fontBuffers = bytes.map(font => Uint8Array.from(font));
    let initialized = initializations.get(initWasm);
    if (!initialized) {
      initialized = initWasm(wasm.buffer).catch(error => { initializations.delete(initWasm); throw error; });
      initializations.set(initWasm, initialized);
    }
    await initialized;
  })().catch(error => { ready = undefined; throw error; });
  return ready;
}

export function createRasterizer(svg: string) {
  return new Resvg(svg, {
    font: { fontBuffers, defaultFontFamily: 'Rajdhani', sansSerifFamily: 'Rajdhani', monospaceFamily: 'IBM Plex Mono' },
    dpi: 96, languages: ['en'], shapeRendering: 2, textRendering: 2, imageRendering: 0,
  });
}
