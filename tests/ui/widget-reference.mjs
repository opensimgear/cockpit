import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import { referenceViews } from '../../scripts/widget-lvgl-fixtures.mjs';

/** Render the actual shared WidgetView for a pixel reference, independently of
 * the device adapter. This fixture has no input handlers or host authority. */
export async function widgetReferenceHtml(state, views = referenceViews.filter(view => view.state === state)) {
  const component = resolve('packages/companion-ui/src/WidgetView.svelte');
  const compiled = compile(await readFile(component, 'utf8'), { filename: component, generate: 'server', css: 'external' });
  const code = compiled.js.code.replace(/from\s+(['"])(\.[^'"]+)\1/g, (_, quote, path) => `from ${quote}${pathToFileURL(resolve(dirname(component), path)).href}${quote}`);
  const directory = resolve('.research/widget-browser-reference'); await mkdir(directory, { recursive: true });
  const output = resolve(directory, 'WidgetView.mjs'); await writeFile(output, code);
  const { default: WidgetView } = await import(pathToFileURL(output).href + '?source=' + Date.now());
  let css = compiled.css.code;
  for (const face of ['sans', 'mono']) css = css.replaceAll(`../assets/fonts/osg-${face}.ttf`, `data:font/ttf;base64,${(await readFile(`packages/companion-ui/assets/fonts/osg-${face}.ttf`)).toString('base64')}`);
  const body = views.map(({ widget, ...props }) => {
    const b = widget.bounds, svg = render(WidgetView, { props: { ...props, now: props.now ?? 100 }, idPrefix: widget.id }).body;
    return `<svg x="${b.x * 800}" y="${b.y * 480}" width="${b.width * 800}" height="${b.height * 480}">${svg}</svg>`;
  }).join('');
  return `<style>html,body{margin:0;padding:0;background:#0d1720}svg{display:block}${css}</style><svg xmlns="http://www.w3.org/2000/svg" width="800" height="480"><rect width="800" height="480" fill="#0d1720"/>${body}</svg>`;
}
