import { COMPANION_LIMITS, validatePortableUi } from "../../companion-contracts/src/index.ts";
import { DESIGNER_LIMITS } from "./types.ts";
import type { AssetPayload, DesignerDocument, PortableBundle, Publication, WidgetLibraryBundle } from "./types.ts";
import { assert, canonical, checked, clone, digest, exact, newId, validateDesignerDocument } from "./model.ts";
import { migratePortableUi } from "./editor.ts";
import { widgetSemantics, validateWidgetLibrary, validateWidgetDefinition, type WidgetDefinition, type WidgetPropertyValue } from "./widget-model.ts";
export function encodeBase64(bytes: Uint8Array): string {
  let binary = ""; for (let i = 0; i < bytes.length; i += 32768) binary += String.fromCharCode(...bytes.subarray(i, i + 32768)); return btoa(binary);
}
export function decodeBase64(value: string): Uint8Array {
  assert(typeof value === "string" && value.length <= Math.ceil(COMPANION_LIMITS.assetBytes / 3) * 4 && value.length % 4 === 0 && /^[A-Za-z0-9+/]*={0,2}$/.test(value), "Invalid or oversized base64 asset");
  const binary = atob(value); const bytes = Uint8Array.from(binary, character => character.charCodeAt(0)); assert(encodeBase64(bytes) === value, "Noncanonical base64 asset"); return bytes;
}
export function imageDimensions(bytes: Uint8Array, mime: string): { width: number; height: number } {
  assert(bytes.length > 0 && bytes.length <= COMPANION_LIMITS.assetBytes, "Asset exceeds byte budget");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); let width = 0, height = 0;
  const ascii = (start: number, size: number) => String.fromCharCode(...bytes.slice(start, start + size));
  if (mime === "image/png") {
    assert(bytes.length >= 33 && bytes.slice(0, 8).every((byte, index) => byte === [137, 80, 78, 71, 13, 10, 26, 10][index]) && ascii(12, 4) === "IHDR" && view.getUint32(8) === 13, "Invalid PNG header");
    width = view.getUint32(16); height = view.getUint32(20);
    let position = 8, ended = false;
    while (position + 12 <= bytes.length) {
      const size = view.getUint32(position), type = ascii(position + 4, 4);
      assert(position + size + 12 <= bytes.length, "Malformed PNG chunk");
      assert(type !== "acTL" && type !== "fcTL" && type !== "fdAT", "Animated PNG assets are unsupported");
      position += size + 12;
      if (type === "IEND") { assert(size === 0 && position === bytes.length, "Malformed PNG end"); ended = true; break; }
    }
    assert(ended, "Missing PNG end");
  } else if (mime === "image/jpeg") {
    assert(bytes.length >= 12 && bytes[0] === 255 && bytes[1] === 216 && bytes.at(-2) === 255 && bytes.at(-1) === 217, "Invalid JPEG header");
    let position = 2;
    while (position + 4 < bytes.length) {
      assert(bytes[position++] === 255, "Malformed JPEG marker"); const marker = bytes[position++];
      if (marker === 0xDA || marker === 0xD9) break;
      const size = view.getUint16(position); assert(size >= 2 && position + size <= bytes.length, "Malformed JPEG segment");
      if ([0xC0, 0xC1, 0xC2].includes(marker)) { assert(size >= 8, "Malformed JPEG dimensions"); height = view.getUint16(position + 3); width = view.getUint16(position + 5); break; }
      position += size;
    }
  } else if (mime === "image/webp") {
    assert(bytes.length >= 30 && ascii(0, 4) === "RIFF" && ascii(8, 4) === "WEBP" && view.getUint32(4, true) + 8 === bytes.length, "Invalid WebP header");
    const type = ascii(12, 4);
    if (type === "VP8X") { assert((bytes[20] & 2) === 0, "Animated WebP assets are unsupported"); width = 1 + bytes[24] + (bytes[25] << 8) + (bytes[26] << 16); height = 1 + bytes[27] + (bytes[28] << 8) + (bytes[29] << 16); }
    else if (type === "VP8L") { assert(bytes[20] === 0x2f, "Invalid WebP lossless header"); width = 1 + bytes[21] + ((bytes[22] & 63) << 8); height = 1 + (bytes[22] >> 6) + (bytes[23] << 2) + ((bytes[24] & 15) << 10); }
    else if (type === "VP8 ") { assert(bytes[23] === 0x9d && bytes[24] === 1 && bytes[25] === 0x2a, "Invalid WebP lossy header"); width = view.getUint16(26, true) & 0x3fff; height = view.getUint16(28, true) & 0x3fff; }
    else throw new Error("Unsupported WebP encoding");
  } else throw new Error("Only PNG, JPEG and WebP assets are supported");
  assert(width > 0 && height > 0 && width <= DESIGNER_LIMITS.assetDimension && height <= DESIGNER_LIMITS.assetDimension && width * height <= DESIGNER_LIMITS.assetPixels, "Asset decoded dimensions exceed budget");
  return { width, height };
}
async function verifyDecodedBitmap(bytes: Uint8Array, mime: string): Promise<void> {
  const expected = imageDimensions(bytes, mime);
  if (typeof globalThis.createImageBitmap !== "function") return;
  const copy = Uint8Array.from(bytes);
  const bitmap = await globalThis.createImageBitmap(new Blob([copy.buffer], { type: mime }), { imageOrientation: "none" });
  try {
    assert(bitmap.width === expected.width && bitmap.height === expected.height && bitmap.width * bitmap.height <= DESIGNER_LIMITS.assetPixels, "Decoded bitmap dimensions mismatch");
  } finally { bitmap.close(); }
}
export async function validateAssets(document: Pick<DesignerDocument, "ui">, payloads: AssetPayload[]): Promise<AssetPayload[]> {
  assert(Array.isArray(payloads) && payloads.length === document.ui.assets.length, "Asset payloads do not match manifest");
  const ids = new Set<string>(); let total = 0, pixels = 0;
  const images: { bytes: Uint8Array; mime: string }[] = [];
  for (const value of payloads) {
    const payload = exact(value, ["assetId", "base64"]); assert(typeof payload.assetId === "string" && !ids.has(payload.assetId), "Duplicate asset payload"); ids.add(payload.assetId);
    const manifest = document.ui.assets.find(asset => asset.id === payload.assetId); assert(manifest, "Unknown asset payload");
    const bytes = decodeBase64(payload.base64 as string); total += bytes.length; assert(total <= COMPANION_LIMITS.totalAssetBytes, "Asset total exceeds budget");
    assert(bytes.length === manifest.sizeBytes && await digest(bytes) === manifest.sha256, "Asset size or digest mismatch");
    const dimensions = imageDimensions(bytes, manifest.mime); pixels += dimensions.width * dimensions.height;
    assert(pixels <= DESIGNER_LIMITS.totalAssetPixels, "Asset total decoded pixels exceed budget");
    images.push({ bytes, mime: manifest.mime });
  }
  for (const image of images) await verifyDecodedBitmap(image.bytes, image.mime);
  return clone(payloads);
}
/** Asset-bearing widget libraries use the same digest, codec and decoded-pixel checks as panels. */
export async function validateWidgetBundle(input: unknown): Promise<WidgetLibraryBundle> {
  const serialized = JSON.stringify(input); assert(serialized && new TextEncoder().encode(serialized).length <= DESIGNER_LIMITS.bundleBytes, "Widget bundle exceeds budget");
  const bundle = exact(JSON.parse(serialized), ["bundleVersion", "library", "manifest", "assets"]); assert(bundle.bundleVersion === 1, "Unsupported widget bundle version");
  const library = validateWidgetLibrary(bundle.library); assert(library.ok, library.ok ? "" : library.error);
  const ui = validatePortableUi({ schemaVersion: 1, id: "widget-assets", title: "Widget assets", controls: [], feedbackWidgets: [], pages: [{ id: "assets-page", title: "Assets", items: [] }], assets: bundle.manifest }, { allowEmptyControls: true }); assert(ui.ok, ui.ok ? "" : ui.error);
  for (const definition of library.value.definitions) { const checked = validateWidgetDefinition(definition, ui.value.assets.map(a => a.id)); assert(checked.ok, checked.ok ? "" : checked.error); }
  return { bundleVersion: 1, library: library.value, manifest: ui.value.assets, assets: await validateAssets({ ui: ui.value }, bundle.assets as AssetPayload[]) };
}
export async function exportWidgetBundle(bundle: WidgetLibraryBundle): Promise<string> { return JSON.stringify(await validateWidgetBundle(bundle), null, 2); }
export async function importWidgetBundle(serialized: string): Promise<WidgetLibraryBundle> {
  assert(typeof serialized === "string" && serialized.length <= DESIGNER_LIMITS.bundleBytes && new TextEncoder().encode(serialized).length <= DESIGNER_LIMITS.bundleBytes, "Widget bundle exceeds budget");
  return validateWidgetBundle(JSON.parse(serialized));
}
export async function mergeWidgetBundles(previous: WidgetLibraryBundle, incoming: WidgetLibraryBundle): Promise<WidgetLibraryBundle> {
  const before = await validateWidgetBundle(previous), next = await validateWidgetBundle(incoming);
  for (const definition of next.library.definitions) {
    const old = before.library.definitions.find(d => d.id === definition.id && d.version === definition.version);
    assert(!old || canonical(old) === canonical(definition), "Widget revisions are immutable; imported revision conflicts with the library");
    if (!old) before.library.definitions.push(definition);
  }
  for (const manifest of next.manifest) {
    const old = before.manifest.find(a => a.id === manifest.id); assert(!old || canonical(old) === canonical(manifest), "Widget asset identity conflict");
    if (!old) { before.manifest.push(manifest); before.assets.push(next.assets.find(a => a.assetId === manifest.id)!); }
  }
  return validateWidgetBundle(before);
}
export function widgetAssetIds(definition: WidgetDefinition, overrides: Record<string, WidgetPropertyValue> = {}): string[] {
  const ids = new Set<string>();
  for (const p of definition.properties) if (p.type === "asset") { if (p.default) ids.add(p.default); if (typeof overrides[p.name] === "string" && overrides[p.name]) ids.add(overrides[p.name] as string); }
  for (const layer of definition.layers) if (layer.kind === "image" && typeof layer.assetId === "string" && layer.assetId) ids.add(layer.assetId);
  return [...ids].sort();
}
export async function importAsset(document: DesignerDocument, bytes: Uint8Array, mime: "image/png" | "image/jpeg" | "image/webp"): Promise<{ document: DesignerDocument; asset: AssetPayload; width: number; height: number }> {
  const dimensions = imageDimensions(bytes, mime), doc = checked(document); await verifyDecodedBitmap(bytes, mime);
  const assetId = newId("asset"), sha256 = await digest(bytes);
  doc.ui.assets.push({ id: assetId, sha256, sizeBytes: bytes.length, mime });
  return { document: checked(doc), asset: { assetId, base64: encodeBase64(bytes) }, ...dimensions };
}
export async function exportBundle(document: DesignerDocument, assets: AssetPayload[]): Promise<string> {
  const doc = checked(document), payloads = await validateAssets(doc, assets);
  const bundle: PortableBundle = { bundleVersion: 1, document: doc, assets: payloads };
  const serialized = JSON.stringify(bundle, null, 2); assert(new TextEncoder().encode(serialized).length <= DESIGNER_LIMITS.bundleBytes, "Bundle exceeds budget"); return serialized;
}
export async function importBundle(serialized: string): Promise<PortableBundle> {
  assert(typeof serialized === "string" && serialized.length <= DESIGNER_LIMITS.bundleBytes && new TextEncoder().encode(serialized).length <= DESIGNER_LIMITS.bundleBytes, "Bundle exceeds budget");
  const parsed = JSON.parse(serialized);
  if (parsed.schemaVersion === 1) {
    const document = migratePortableUi(parsed); assert(document.ui.assets.length === 0, "Legacy document needs an asset bundle"); return { bundleVersion: 1, document, assets: [] };
  }
  const bundle = exact(parsed, ["bundleVersion", "document", "assets"]); assert(bundle.bundleVersion === 1, "Unsupported bundle version");
  const result = validateDesignerDocument(bundle.document); assert(result.ok, result.ok ? "" : result.error);
  return { bundleVersion: 1, document: result.value, assets: await validateAssets(result.value, bundle.assets as AssetPayload[]) };
}
function capabilityData(document: DesignerDocument): unknown {
  const doc = checked(document);
  return {
    uiId: doc.ui.id,
    controls: doc.ui.controls.map(({ id, kind, ...control }) => ({ id, kind, ...(kind === "axis" ? { axisMode: (control as { axisMode: string }).axisMode, fallback: (control as { fallback: number }).fallback } : {}) })).sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
    feedback: doc.ui.feedbackWidgets.map(({ id, kind, channelId }) => ({ id, kind, channelId })).sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
    widgets: doc.presentation.widgets.map(widget => {
      if (widget.kind === "component") {
        const definition = doc.presentation.definitions!.find(d => d.id === widget.config.definitionId && d.version === widget.config.definitionVersion)!;
        return { id: widget.id, kind: widget.kind, controlIds: widget.controlIds, feedbackIds: widget.feedbackIds, config: widgetSemantics(definition, widget.config.properties) };
      }
      const config = clone(widget.config) as Record<string, unknown>;
      for (const key of ["assetId", "iconAssetId", "symbol", "prefix", "suffix", "orientation"]) delete config[key];
      if (widget.kind === "selector") config.options = widget.config.options.map(option => ({ value: option.value }));
      return { id: widget.id, kind: widget.kind, controlIds: widget.controlIds, feedbackIds: widget.feedbackIds, config };
    }).sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
    targets: doc.targets.map(({ id, axes, buttons, hats }) => ({ id, axes, buttons, hats })).sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
    bindings: [...doc.bindings].sort((a, b) => a.controlId < b.controlId ? -1 : a.controlId > b.controlId ? 1 : 0),
  };
}
export function compareDesignerPublications(previous: DesignerDocument, next: DesignerDocument): "identical" | "styling" | "capability" {
  const first = checked(previous), second = checked(next);
  if (canonical(first) === canonical(second)) return "identical";
  return canonical(capabilityData(first)) === canonical(capabilityData(second)) ? "styling" : "capability";
}
function freeze<T>(value: T): T {
  if (value && typeof value === "object") { for (const item of Object.values(value)) freeze(item); Object.freeze(value); }
  return value;
}
export async function publishDocument(document: DesignerDocument, assets: AssetPayload[], options: { optimizeAssets?: boolean } = {}): Promise<Publication> {
  const doc = checked(document); assert(doc.bindings.length === doc.ui.controls.length, "Every input requires a logical target binding before publication");
  assert(doc.ui.pages.every(page=>doc.presentation.widgets.some(widget=>widget.pageId===page.id)), "Every page must contain at least one widget before publication");
  const validated = await validateAssets(doc, assets);
  const payloads = options.optimizeAssets === false ? validated : await optimizePublicationAssets(doc, validated), publicationHash = "sha256:" + await digest(new TextEncoder().encode(canonical(doc)));
  const capabilityFingerprint = "sha256:" + await digest(new TextEncoder().encode(canonical(capabilityData(doc))));
  return freeze({ document: doc, assets: payloads, publicationHash, capabilityFingerprint });
}

/** Retain source assets in drafts; publish only the resolution the layout needs. */
async function optimizePublicationAssets(doc:DesignerDocument, assets:AssetPayload[]):Promise<AssetPayload[]>{
  if(typeof createImageBitmap!=='function' || typeof OffscreenCanvas==='undefined')return assets;
  const result:AssetPayload[]=[];
  for(const payload of assets){
    const manifest=doc.ui.assets.find(a=>a.id===payload.assetId)!;
    let width=0,height=0;
    for(const widget of doc.presentation.widgets){
      const config=widget.config as Record<string,unknown>;
      if(config.assetId!==payload.assetId && config.iconAssetId!==payload.assetId)continue;
      const screen=doc.presentation.viewports.find(v=>v.pageId===widget.pageId)!;
      width=Math.max(width,Math.ceil(widget.bounds.width*screen.width*(1-(screen.safeInsets?.left??0)-(screen.safeInsets?.right??0))));
      height=Math.max(height,Math.ceil(widget.bounds.height*screen.height*(1-(screen.safeInsets?.top??0)-(screen.safeInsets?.bottom??0))));
    }
    // Legacy background publications remain readable until opened in the editor.
    for(const screen of doc.presentation.viewports)if(screen.backgroundAssetId===payload.assetId){width=Math.max(width,screen.width);height=Math.max(height,screen.height);}
    if(!width||!height){result.push(payload);continue;}
    const bytes=decodeBase64(payload.base64),bitmap=await createImageBitmap(new Blob([Uint8Array.from(bytes).buffer],{type:manifest.mime}),{imageOrientation:'none'});
    try{
      const scale=Math.min(1,Math.max(width/bitmap.width,height/bitmap.height));
      const isImage=doc.presentation.widgets.some(w=>w.kind==='image' && w.config.assetId===payload.assetId);
      const targetWidth=isImage?Math.min(width,bitmap.width):Math.max(1,Math.ceil(bitmap.width*scale)),targetHeight=isImage?Math.min(height,bitmap.height):Math.max(1,Math.ceil(bitmap.height*scale));
      if(targetWidth===bitmap.width && targetHeight===bitmap.height){result.push(payload);continue;}
      const canvas=new OffscreenCanvas(targetWidth,targetHeight),ctx=canvas.getContext('2d');assert(ctx,'Image resize unavailable');
      ctx.imageSmoothingEnabled=true;ctx.imageSmoothingQuality='high';ctx.drawImage(bitmap,0,0,canvas.width,canvas.height);
      const blob=await canvas.convertToBlob({type:manifest.mime==='image/jpeg'?'image/jpeg':'image/png',quality:.92}),optimized=new Uint8Array(await blob.arrayBuffer());
      manifest.mime=blob.type as typeof manifest.mime;manifest.sizeBytes=optimized.length;manifest.sha256=await digest(optimized);
      result.push({assetId:payload.assetId,base64:encodeBase64(optimized)});
    }finally{bitmap.close();}
  }
  return result;
}
