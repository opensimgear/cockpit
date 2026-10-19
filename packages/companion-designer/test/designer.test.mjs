import test from "node:test";
import assert from "node:assert/strict";
import { validateAssets, canonical } from "../src/index.ts";
import { newDocument, createDesignerHistory, applyDesignerCommand as apply, undoDesigner, redoDesigner, widgetPresets, validateDesignerDocument as validate, normalizeToPortableUi, migratePortableUi, flightTemplate, racingTemplate, compareDesignerPublications, publishDocument, importAsset, exportBundle, importBundle, imageDimensions, encodeBase64, decodeBase64, createLibrary, saveDraft, openDraft, duplicateDraft, deleteDraft, validateLibrary, DESIGNER_LIMITS as L } from "../src/index.ts";
import { exampleUi } from "../../companion-contracts/src/index.ts";
const clone = value => structuredClone(value);
const seed = () => newDocument("Test panel");
const history = () => createDesignerHistory(seed());
const first = doc => doc.presentation.widgets[0];
function withKind(kind) { return apply(history(), { type: "add-widget", kind }).present; }

test("capability identity uses portable ordinal ID order rather than locale collation", async () => {
  let h = history(); h = apply(h, { type: "add-widget", kind: "button" });
  const doc = h.present, oldIds = doc.ui.controls.map(control => control.id);
  for (let i = 0; i < oldIds.length; i++) {
    const replacement = i === 0 ? "input-Z" : "input-a";
    doc.ui.controls[i].id = replacement;
    doc.bindings.find(binding => binding.controlId === oldIds[i]).controlId = replacement;
    doc.presentation.widgets[i].controlIds = [replacement];
  }
  doc.ui = normalizeToPortableUi(doc);
  const previous = String.prototype.localeCompare;
  String.prototype.localeCompare = () => { throw new Error("Locale sorting is not a protocol operation"); };
  try {
    const published = await publishDocument(doc, []);
    const reversed = clone(doc); reversed.ui.controls.reverse(); reversed.bindings.reverse();
    assert.equal((await publishDocument(reversed, [])).capabilityFingerprint, published.capabilityFingerprint);
  } finally { String.prototype.localeCompare = previous; }
});

test("publication JSON preserves ECMAScript small-number and negative-zero canonical spelling", async () => {
  const doc = withKind("slider");
  const axis = doc.ui.controls.find(control => control.kind === "axis"); axis.fallback = 1e-7;
  const widget = doc.presentation.widgets.find(widget => widget.kind === "slider"); widget.bounds.x = 1e-6; widget.bounds.y = -0;
  doc.ui = normalizeToPortableUi(doc);
  const publication = await publishDocument(doc, []);
  const roundtrip = await importBundle(await exportBundle(doc, []));
  assert.equal((await publishDocument(roundtrip.document, [])).publicationHash, publication.publicationHash);
  assert.equal(Object.is(publication.document.presentation.widgets.find(widget => widget.kind === "slider").bounds.y, -0), false);
  assert.equal(publication.document.ui.controls.find(control => control.kind === "axis").fallback, 1e-7);
});

test("representative flight/racing fixtures are stable, valid, normalized and portable", () => {
  for (const document of [flightTemplate, racingTemplate]) {
    assert.equal(validate(document).ok, true);
    assert.deepEqual(normalizeToPortableUi(document), document.ui);
    assert.ok(document.bindings.length === document.ui.controls.length);
  }
  assert.equal(flightTemplate.ui.id, "flight-panel"); assert.equal(racingTemplate.ui.id, "racing-panel");
  assert.equal(flightTemplate.presentation.widgets.find(w => w.kind === "button" && w.config.buttonMode === "toggle")?.label, "Button");
  assert.ok(racingTemplate.ui.controls.some(control => control.kind === "axis" && control.axisMode === "unipolar"));
  for (const preset of widgetPresets) assert.equal(validate(withKind(preset.kind)).ok, true, preset.kind);
});
test("immutable editing preserves IDs through movement, styling and undo/redo", () => {
  let h = history(), old = clone(h.present), id = first(old).id, control = first(old).controlIds[0];
  h = apply(h, { type: "move-widget", id, bounds: { x: 0.1, y: 0.2, width: 0.4, height: 0.2 } });
  h = apply(h, { type: "update-widget", id, patch: { label: "Ignition", style: { accent: "#123456" } } });
  assert.equal(first(h.present).controlIds[0], control); assert.equal(first(h.present).id, id);
  assert.equal(first(old).label, "Button"); assert.equal(first(undoDesigner(h).present).label, "Button");
  assert.deepEqual(redoDesigner(undoDesigner(h)).present, h.present);
  assert.throws(() => apply(h, { type: "update-widget", id, patch: { id: "changed" } }), /Stable/);
  assert.throws(() => apply(h, { type: "move-widget", id, bounds: { x: 1, y: 0, width: 0.4, height: 0.2 } }), /outside page/);
  const restored = undoDesigner(undoDesigner(h)); assert.deepEqual(restored.present, old);
});
test("history is bounded and edits discard redo without mutating earlier states", () => {
  let h = history(); const id = first(h.present).id;
  for (let i = 0; i < 55; i++) h = apply(h, { type: "update-widget", id, patch: { label: "Button " + i } });
  assert.equal(h.past.length, 50); const undone = undoDesigner(h); assert.equal(undone.future.length, 1);
  const fork = apply(undone, { type: "update-widget", id, patch: { label: "Fork" } }); assert.equal(fork.future.length, 0);
  assert.deepEqual(redoDesigner(history()).future, []); assert.deepEqual(undoDesigner(history()).past, []);
});
test("duplication remaps controls and outputs while preserving group membership", () => {
  let h = history(); const original = first(h.present);
  h = apply(h, { type: "add-group", pageId: original.pageId, label: "Actions", widgetIds: [original.id] });
  h = apply(h, { type: "duplicate-widget", id: original.id });
  const [a, b] = h.present.presentation.widgets;
  assert.notEqual(a.id, b.id); assert.notEqual(a.controlIds[0], b.controlIds[0]); assert.equal(a.groupId, b.groupId);
  assert.notEqual(h.present.bindings[0].index, h.present.bindings[1].index);
  h = apply(h, { type: "add-widget", kind: "numeric" }); const output = h.present.presentation.widgets.at(-1);
  h = apply(h, { type: "duplicate-widget", id: output.id }); assert.notEqual(h.present.presentation.widgets.at(-1).feedbackIds[0], output.feedbackIds[0]);
  h = apply(h, { type: "ungroup", id: a.groupId }); assert.ok(h.present.presentation.widgets.every(widget => !widget.groupId));
  assert.throws(() => apply(h, { type: "ungroup", id: "unknown" }));
});
test("grid, alignment and group translation stay normalized", () => {
  let h = apply(history(), { type: "add-widget", kind: "button", bounds: { x: 0.41, y: 0.38, width: 0.2, height: 0.2 } });
  const ids = h.present.presentation.widgets.map(widget => widget.id);
  for (const edge of ["left", "right", "top", "bottom", "center-x", "center-y"]) {
    const aligned = apply(h, { type: "align", ids, edge }); assert.equal(validate(aligned.present).ok, true);
  }
  h = apply(h, { type: "set-grid", columns: 10, rows: 10, snap: true });
  h = apply(h, { type: "snap", ids }); assert.equal(h.present.presentation.widgets[1].bounds.x, 0.4);
  h = apply(h, { type: "translate", ids, dx: 0.1, dy: 0.1 }); assert.equal(validate(h.present).ok, true);
  assert.throws(() => apply(h, { type: "translate", ids, dx: 5, dy: 0 }));
  assert.throws(() => apply(h, { type: "snap", ids: ["missing"] }));
});

test("left and right alignment use outer selection edges regardless of selection order or widget width", () => {
  let h=history();const original=first(h.present).id;
  h=apply(h,{type:'update-widget',id:original,patch:{bounds:{x:0.1,y:0.1,width:0.2,height:0.2}}});
  h=apply(h,{type:'add-widget',kind:'button',bounds:{x:0.5,y:0.4,width:0.3,height:0.2}});
  const added=h.present.presentation.widgets.at(-1).id;
  for(const ids of [[original,added],[added,original]]){
    const left=apply(h,{type:'align',ids,edge:'left'}).present.presentation.widgets;
    assert.ok(left.every(widget=>Math.abs(widget.bounds.x-0.1)<1e-10));
    const right=apply(h,{type:'align',ids,edge:'right'}).present.presentation.widgets;
    assert.ok(right.every(widget=>Math.abs(widget.bounds.x+widget.bounds.width-0.8)<1e-10));
    assert.deepEqual(right.map(widget=>widget.bounds.width),[0.2,0.3]);
    const top=apply(h,{type:'align',ids,edge:'top'}).present.presentation.widgets;
    assert.ok(top.every(widget=>Math.abs(widget.bounds.y-0.1)<1e-10));
    const bottom=apply(h,{type:'align',ids,edge:'bottom'}).present.presentation.widgets;
    assert.ok(bottom.every(widget=>Math.abs(widget.bounds.y+widget.bounds.height-0.6)<1e-10));
  }
});
test("page/viewport operations and deletion clean references atomically", () => {
  let h = history(); const original = clone(h.present);
  h = apply(h, { type: "add-page", title: "Engine" }); const pageId = h.present.ui.pages[1].id;
  h = apply(h, { type: "set-viewport", pageId, width: 800, height: 600, safeInsets: { top: 0.05, right: 0, bottom: 0, left: 0 } });
  h = apply(h, { type: "add-widget", kind: "slider", pageId, axisMode: "unipolar" }); const widget = h.present.presentation.widgets.at(-1);
  h = apply(h, { type: "add-group", pageId, label: "Engine", widgetIds: [widget.id] });
  assert.throws(() => apply(h, { type: "align", ids: [first(h.present).id, widget.id], edge: "left" }), /spans/);
  h = apply(h, { type: "delete-page", id: pageId }); assert.equal(h.present.ui.pages.length, 1); assert.equal(h.present.presentation.groups.length, 0);
  assert.equal(h.present.ui.controls.length, original.ui.controls.length);
  assert.throws(() => apply(h, { type: "delete-page", id: h.present.ui.pages[0].id }));
  assert.throws(() => apply(h, { type: "delete-widget", id: first(h.present).id }));
});
test("default page numbering starts at zero and closes gaps after deletion with undo", () => {
  let h=history();
  assert.equal(h.present.ui.pages[0].title,'Page 0');
  h=apply(h,{type:'add-page',title:'Page 1'});
  h=apply(h,{type:'add-page',title:'Page 2'});
  h=apply(h,{type:'add-widget',kind:'button',pageId:h.present.ui.pages[1].id});
  const remainingIds=h.present.ui.pages.slice(1).map(page=>page.id);
  h=apply(h,{type:'delete-page',id:h.present.ui.pages[0].id});
  assert.deepEqual(h.present.ui.pages.map(page=>page.title),['Page 0','Page 1']);
  assert.deepEqual(h.present.ui.pages.map(page=>page.id),remainingIds);
  h=undoDesigner(h);
  assert.deepEqual(h.present.ui.pages.map(page=>page.title),['Page 0','Page 1','Page 2']);
  h=redoDesigner(h);
  assert.deepEqual(h.present.ui.pages.map(page=>page.title),['Page 0','Page 1']);
});

test("logical targets enforce range/index compatibility independently of machine assignments", () => {
  let h = apply(history(), { type: "add-widget", kind: "slider" }); const axis = h.present.ui.controls.find(c => c.kind === "axis"), target = h.present.targets[0];
  h = apply(h, { type: "unbind", controlId: axis.id });
  assert.throws(() => apply(h, { type: "bind", controlId: axis.id, targetId: target.id, kind: "axis", index: 99 }));
  h = apply(h, { type: "bind", controlId: axis.id, targetId: target.id, kind: "axis", index: 0 });
  assert.throws(() => apply(h, { type: "update-target", id: target.id, patch: { axes: ["unipolar", "bipolar"] } }), /range/);
  h = apply(h, { type: "add-target" }); const second = h.present.targets[1];
  h = apply(h, { type: "remove-target", id: second.id }); assert.equal(h.present.targets.length, 1);
  h = apply(h, { type: "remove-target", id: target.id }); assert.equal(h.present.bindings.length, 0);
});
test("legacy migration preserves all logical IDs and explicitly rejects unsupported data", async () => {
  const document = migratePortableUi(exampleUi);
  assert.deepEqual(document.ui.controls, exampleUi.controls); assert.deepEqual(document.ui.feedbackWidgets, exampleUi.feedbackWidgets);
  assert.equal(document.ui.id, exampleUi.id); assert.equal(validate(document).ok, true);
  assert.equal((await importBundle(JSON.stringify(exampleUi))).document.ui.id, exampleUi.id);
  assert.throws(() => migratePortableUi({ schemaVersion: 99 }));
  await assert.rejects(importBundle(JSON.stringify({ ...exampleUi, credentials: "secret" })));
});
const bad = (name, mutate, make = () => clone(flightTemplate)) => test("strict document rejects " + name, () => { const doc = make(); mutate(doc); assert.equal(validate(doc).ok, false); });
for (const [name, mutate] of [
  ["unknown local targets", d => d.localTargetSlots = [1]],
  ["credentials", d => d.presentation.pairingSecret = "secret"],
  ["schema", d => d.schemaVersion = 3],
  ["duplicate widget", d => d.presentation.widgets[0].id = d.presentation.widgets[1].id],
  ["unbound primitive", d => d.presentation.widgets.shift()],
  ["wrong primitive type", d => d.presentation.widgets[0].controlIds = [d.ui.controls.find(c => c.kind === "axis").id]],
  ["unknown page", d => d.presentation.widgets[0].pageId = "missing"],
  ["group reference", d => d.presentation.widgets[0].groupId = "missing"],
  ["viewport size", d => d.presentation.viewports[0].width = 8193],
  ["safe insets", d => d.presentation.viewports[0].safeInsets = { top: 0.4, right: 0, bottom: 0, left: 0 }],
  ["viewport duplicate", d => d.presentation.viewports.push(d.presentation.viewports[0])],
  ["grid", d => d.presentation.grid.columns = 0],
  ["bounds", d => d.presentation.widgets[0].bounds.width = 0],
  ["style", d => d.presentation.widgets[0].style = { css: "url(secret)" }],
  ["target kind", d => d.targets[0].axes[0] = "raw"],
  ["target empty", d => Object.assign(d.targets[0], { axes: [], buttons: 0, hats: 0 })],
  ["binding duplicate slot", d => d.bindings[1] = { ...d.bindings[0], controlId: d.bindings[1].controlId }],
  ["output arity", d => d.presentation.widgets.find(w => w.kind === "text").feedbackIds = []],
  ["UI byte budget", d => d.ui.title = "x".repeat(1024 * 1024)],
]) bad(name, mutate);
for (const [name, kind, patch] of [
  ["button mode", "button", { buttonMode: "invalid" }], ["toggle repeating", "button", { buttonMode: "toggle", repeatMs: 50 }],
  ["pulse missing", "button", { buttonMode: "pulse", pulseMs: 0 }], ["repeat interval", "button", { buttonMode: "repeat", pulseMs: 50, repeatMs: 10 }],
  ["selector duplicate", "selector", { options: [{ label: "A", value: 0 }, { label: "B", value: 0 }] }],
  ["selector fallback", "selector", { options: [{ label: "A", value: 0.2 }, { label: "B", value: 0.8 }] }],
  ["encoder pulse", "encoder", { pulseMs: 0 }], ["encoder repeat", "encoder", { pulseMs: 100, repeatMs: 50 }],
  ["deadzone", "slider", { deadzone: 0.5 }], ["centering", "xy", { centering: "sticky" }],
  ["orientation", "slider", { orientation: "diagonal" }], ["inversion", "xy", { invertX: 1 }],
  ["numeric range", "numeric", { min: 100, max: 0 }], ["precision", "gauge", { precision: 5 }],
  ["icon URL", "icon", { symbol: "https://bad" }], ["icon missing asset", "icon", { iconAssetId: "missing" }],
]) bad(name, d => { const widget = d.presentation.widgets.find(w => w.kind === kind); Object.assign(widget.config, patch); });
test("publication hashes outer semantics and separates styling from capability changes", async () => {
  const doc = clone(flightTemplate), publication = await publishDocument(doc, []);
  assert.ok(Object.isFrozen(publication.document.presentation.widgets[0]));
  const styled = clone(doc); first(styled).label = "New label"; styled.ui = normalizeToPortableUi(styled);
  assert.equal(compareDesignerPublications(doc, styled), "styling");
  const semantics = clone(doc); first(semantics).config.buttonMode = "toggle";
  assert.equal(compareDesignerPublications(doc, semantics), "capability");
  assert.notEqual((await publishDocument(semantics, [])).publicationHash, publication.publicationHash);
  assert.notEqual((await publishDocument(semantics, [])).capabilityFingerprint, publication.capabilityFingerprint);
  assert.equal(compareDesignerPublications(doc, clone(doc)), "identical");
  doc.ui.title = "Later edit"; assert.notEqual(publication.document.ui.title, doc.ui.title);
  const unbound = seed(); unbound.bindings = []; await assert.rejects(publishDocument(unbound, []), /binding/);
});
test("library save/reopen/duplicate/delete detaches state and remaps all bindings", async () => {
  let library = await saveDraft(createLibrary(), { id: "draft-a", name: "Original", document: clone(flightTemplate), assets: [] });
  const reopened = openDraft(library, "draft-a"); reopened.name = "edit"; assert.equal(library.drafts[0].name, "Original");
  library = duplicateDraft(library, "draft-a"); assert.equal(library.drafts.length, 2);
  assert.notEqual(library.drafts[0].document.ui.id, library.drafts[1].document.ui.id);
  assert.ok(library.drafts[1].document.bindings.every(binding => library.drafts[1].document.ui.controls.some(control => control.id === binding.controlId)));
  assert.equal((await validateLibrary(library)).ok, true);
  library = await saveDraft(library, { ...library.drafts[0], name: "Updated" }); assert.equal(library.drafts.length, 2);
  library = deleteDraft(library, library.drafts[1].id); assert.equal(library.drafts.length, 1);
  assert.throws(() => openDraft(library, "missing")); assert.throws(() => deleteDraft(library, "missing"));
  assert.equal((await validateLibrary({ ...library, password: "no" })).ok, false);
  assert.equal((await validateLibrary({ ...library, drafts: [library.drafts[0], library.drafts[0]] })).ok, false);
});
const png = () => Uint8Array.from(atob("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="), c => c.charCodeAt(0));
test("the complete asset set must fit the decoded pixel budget before browser decode", async () => {
  const bytes = png(), view = new DataView(bytes.buffer); view.setUint32(16, 4096); view.setUint32(20, 4096);
  const sha256 = Buffer.from(await crypto.subtle.digest("SHA-256", bytes)).toString("hex");
  const document = seed(), assets = [0, 1, 2].map(index => ({ assetId: `image-${index}`, base64: encodeBase64(bytes) }));
  document.ui.assets = assets.map(({ assetId }) => ({ id: assetId, sizeBytes: bytes.length, sha256, mime: "image/png" }));
  const oldDecode = globalThis.createImageBitmap; let decoded = 0, closed = 0;
  globalThis.createImageBitmap = async () => { decoded++; return { width: 4096, height: 4096, close() { closed++; } }; };
  try {
    await assert.rejects(validateAssets(document, assets), /total decoded pixels/); assert.equal(decoded, 0);
    document.ui.assets.pop();
    assert.equal((await validateAssets(document, assets.slice(0, 2))).length, 2);
    assert.equal(decoded, 2); assert.equal(closed, 2); assert.equal(L.totalAssetPixels, 2 * 4096 * 4096);
  } finally { globalThis.createImageBitmap = oldDecode; }
});

test("image bundle import/export validates digest, MIME, bytes, dimensions and separate budgets", async () => {
  const imported = await importAsset(seed(), png(), "image/png");
  assert.equal(imported.width, 1); assert.equal(imported.height, 1);
  const doc = imported.document; doc.presentation.viewports[0].backgroundAssetId = imported.asset.assetId;
  first(doc).config.iconAssetId = imported.asset.assetId;
  const serialized = await exportBundle(doc, [imported.asset]), reopened = await importBundle(serialized);
  assert.deepEqual(reopened.document, doc); assert.deepEqual(reopened.assets, [imported.asset]);
  const corrupt = JSON.parse(serialized); corrupt.assets[0].base64 = encodeBase64(new Uint8Array([1, 2, 3]));
  await assert.rejects(importBundle(JSON.stringify(corrupt)), /digest/);
  await assert.rejects(exportBundle(doc, []), /manifest/);
  const huge = png(); new DataView(huge.buffer).setUint32(16, 65535); assert.throws(() => imageDimensions(huge, "image/png"), /dimensions/);
  assert.throws(() => imageDimensions(png(), "image/svg+xml"), /Only/);
  assert.throws(() => decodeBase64("bad=data"));
  await assert.rejects(importBundle(" ".repeat(L.bundleBytes + 1)), /budget/);
  await assert.rejects(importBundle('{"bundleVersion":2,"document":{},"assets":[]}'), /version/);
  const credentials = JSON.parse(serialized); credentials.password = "secret"; await assert.rejects(importBundle(JSON.stringify(credentials)), /fields/);
});
test("JPEG and WebP dimension parsers bound resources before browser image decode", () => {
  const jpeg = new Uint8Array([255,216,255,192,0,8,8,0,2,0,3,0,255,217]);
  assert.deepEqual(imageDimensions(jpeg, "image/jpeg"), { width: 3, height: 2 });
  const webp = new Uint8Array(30), write = (offset, text) => [...text].forEach((c, i) => webp[offset + i] = c.charCodeAt(0));
  write(0, "RIFF"); write(8, "WEBP"); write(12, "VP8X"); new DataView(webp.buffer).setUint32(4, 22, true);
  assert.deepEqual(imageDimensions(webp, "image/webp"), { width: 1, height: 1 });
  write(12, "VP8L"); webp[20] = 0x2f; assert.deepEqual(imageDimensions(webp, "image/webp"), { width: 1, height: 1 });
  write(12, "VP8 "); webp[23] = 0x9d; webp[24] = 1; webp[25] = 0x2a; webp[26] = 2; webp[28] = 3;
  assert.deepEqual(imageDimensions(webp, "image/webp"), { width: 2, height: 3 });
  write(12, "BAD!"); assert.throws(() => imageDimensions(webp, "image/webp"));
  assert.throws(() => imageDimensions(new Uint8Array(0), "image/png"));
});


test("pulse/repeat/latched semantics and selector values change capability fingerprints", async () => {
  for (const [kind, config] of [
    ["button", { buttonMode: "pulse", pulseMs: 30, repeatMs: 0 }],
    ["button", { buttonMode: "repeat", pulseMs: 30, repeatMs: 100 }],
    ["encoder", { pulseMs: 25, repeatMs: 100 }],
    ["slider", { centering: "latched", deadzone: 0.1, inverted: true, orientation: "vertical" }],
    ["xy", { centering: "latched", deadzone: 0.1, invertX: true, invertY: false }],
  ]) {
    const original = withKind(kind), document = clone(original), widget = document.presentation.widgets.at(-1); widget.config = config;
    assert.equal(validate(document).ok, true); assert.equal(compareDesignerPublications(original, document), "capability");
  }
  const original = withKind("selector"), renamed = clone(original); renamed.presentation.widgets.at(-1).config.options[0].label = "Renamed option";
  assert.equal(compareDesignerPublications(original, renamed), "styling");
  const changed = clone(original); changed.presentation.widgets.at(-1).config.options[1].value = 0.25;
  assert.equal(compareDesignerPublications(original, changed), "capability");
});
test("draft duplication remaps groups and asset references without crosslinking originals", async () => {
  const imported = await importAsset(seed(), png(), "image/png");
  let h = createDesignerHistory(imported.document);
  h = apply(h, { type: "add-group", pageId: first(h.present).pageId, label: "Group", widgetIds: [first(h.present).id] });
  first(h.present).config.iconAssetId = imported.asset.assetId;
  h.present.presentation.viewports[0].backgroundAssetId = imported.asset.assetId;
  const library = await saveDraft(createLibrary(), { id: "draft-assets", name: "Assets", document: h.present, assets: [imported.asset] });
  const duplicated = duplicateDraft(library, "draft-assets"), copy = duplicated.drafts[1], original = duplicated.drafts[0];
  assert.notEqual(first(copy.document).groupId, first(original.document).groupId);
  assert.notEqual(first(copy.document).config.iconAssetId, first(original.document).config.iconAssetId);
  assert.equal(copy.document.presentation.viewports[0].backgroundAssetId, copy.assets[0].assetId);
  assert.equal((await validateLibrary(duplicated)).ok, true);
});
test("library capacity and malformed persisted drafts are rejected without changing current state", async () => {
  let library = createLibrary();
  for (let i = 0; i < L.drafts; i++) library = await saveDraft(library, { id: "draft-" + i, name: "Panel", document: seed(), assets: [] });
  await assert.rejects(saveDraft(library, { id: "overflow", name: "Overflow", document: seed(), assets: [] }), /full/);
  assert.throws(() => duplicateDraft(library, library.drafts[0].id), /full/);
  assert.equal(library.drafts.length, L.drafts);
  assert.equal((await validateLibrary({ schemaVersion: 9, drafts: [] })).ok, false);
  assert.equal((await validateLibrary({ schemaVersion: 1, drafts: [{ id: "a", name: "Bad", document: {}, assets: [] }] })).ok, false);
});
test("base64 decoding has a distinct bounded asset budget and no large-string regex recursion", () => {
  const bytes = new Uint8Array(2 * 1024 * 1024); bytes[0] = 1;
  const encoded = encodeBase64(bytes); assert.ok(encoded.length > 1024 * 1024); assert.deepEqual(decodeBase64(encoded), bytes);
  assert.throws(() => decodeBase64(encoded + "AAAA"), /oversized/);
});
test("unknown targets/groups/pages and incompatible logical bindings never produce partial history", () => {
  const h = history(), before = clone(h);
  for (const command of [
    { type: "add-widget", kind: "unknown" },
    { type: "delete-widget", id: "missing" },
    { type: "set-viewport", pageId: "missing", width: 800, height: 600 },
    { type: "update-target", id: "missing", patch: {} },
    { type: "remove-target", id: "missing" },
    { type: "add-group", pageId: "missing", label: "Wrong", widgetIds: [first(h.present).id] },
    { type: "bind", controlId: first(h.present).controlIds[0], targetId: "missing", kind: "button", index: 0 },
  ]) assert.throws(() => apply(h, command));
  assert.deepEqual(h, before);
});

test("browser bitmap validation closes decoded resources and rejects mismatched/corrupt compressed data", async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, "createImageBitmap"); let closed = 0, called = 0;
  try {
    globalThis.createImageBitmap = async (blob, options) => { called++; assert.equal(blob.type, "image/png"); assert.equal(options.imageOrientation, "none"); return { width: 1, height: 1, close() { closed++; } }; };
    const imported = await importAsset(seed(), png(), "image/png"); assert.equal(closed, 1);
    await exportBundle(imported.document, [imported.asset]); assert.equal(closed, 2);
    globalThis.createImageBitmap = async () => ({ width: 9999, height: 9999, close() { closed++; } });
    await assert.rejects(importAsset(seed(), png(), "image/png"), /mismatch/); assert.equal(closed, 3);
    globalThis.createImageBitmap = async () => { throw new Error("Corrupt compressed image"); };
    await assert.rejects(importAsset(seed(), png(), "image/png"), /compressed/); assert.ok(called >= 2);
  } finally {
    if (previous) Object.defineProperty(globalThis, "createImageBitmap", previous); else delete globalThis.createImageBitmap;
  }
});
test("animated and truncated image containers are rejected during bounded preflight", () => {
  const bytes = png(), animated = new Uint8Array(bytes.length + 20);
  animated.set(bytes.subarray(0, 33)); new DataView(animated.buffer).setUint32(33, 8); [..."acTL"].forEach((c,i) => animated[37+i] = c.charCodeAt(0)); animated.set(bytes.subarray(33), 53);
  assert.throws(() => imageDimensions(animated, "image/png"), /Animated/);
  assert.throws(() => imageDimensions(bytes.subarray(0, bytes.length - 1), "image/png"), /PNG/);
  const webp = new Uint8Array(30); [..."RIFF"].forEach((c,i) => webp[i]=c.charCodeAt(0)); [..."WEBP"].forEach((c,i) => webp[8+i]=c.charCodeAt(0)); [..."VP8X"].forEach((c,i) => webp[12+i]=c.charCodeAt(0)); new DataView(webp.buffer).setUint32(4,22,true); webp[20]=2;
  assert.throws(() => imageDimensions(webp, "image/webp"), /Animated/);
});

test("committed example bundles import as the exact representative portable fixtures", async () => {
  const { readFile } = await import("node:fs/promises");
  for (const [file, document] of [["flight-panel.json", flightTemplate], ["racing-dashboard.json", racingTemplate]]) {
    const imported = await importBundle(await readFile(new URL("../examples/" + file, import.meta.url), "utf8"));
    assert.deepEqual(imported.document, document); assert.deepEqual(imported.assets, []);
    assert.equal((await publishDocument(imported.document, [])).publicationHash, (await publishDocument(document, [])).publicationHash);
  }
});

test("repeating buttons and encoders require a positive release interval between pulses", () => {
  for (const kind of ["button", "encoder"]) {
    const document = withKind(kind), widget = document.presentation.widgets.at(-1);
    widget.config = kind === "button" ? { buttonMode: "repeat", pulseMs: 50, repeatMs: 50 } : { pulseMs: 50, repeatMs: 50 };
    assert.equal(validate(document).ok, false);
    widget.config.repeatMs = 51; assert.equal(validate(document).ok, true);
  }
});

test("two distinct authored flight/racing panels each survive pages/assets and the complete library roundtrip", async () => {
  let library = createLibrary();
  for (const [id, template, title] of [["flight-draft", flightTemplate, "Flight with engine page"], ["racing-draft", racingTemplate, "Racing with pit page"]]) {
    let h = createDesignerHistory(template);
    h = apply(h, { type: "add-page", title: id === "flight-draft" ? "Engine" : "Pit" });
    const secondPage = h.present.ui.pages[1].id;
    h = apply(h, { type: "add-widget", kind: "button", pageId: secondPage });
    const image = await importAsset(h.present, png(), "image/png");
    image.document.ui.title = title;
    image.document.presentation.viewports.find(viewport => viewport.pageId === secondPage).backgroundAssetId = image.asset.assetId;
    library = await saveDraft(library, { id, name: title, document: image.document, assets: [image.asset] });
  }
  const persisted = await validateLibrary(clone(library)); assert.equal(persisted.ok, true);
  library = persisted.value;
  assert.notEqual(library.drafts[0].document.ui.id, library.drafts[1].document.ui.id);
  for (const id of ["flight-draft", "racing-draft"]) {
    const reopened = openDraft(library, id);
    assert.equal(reopened.document.ui.pages.length, 2); assert.equal(reopened.assets.length, 1);
    const originalId = reopened.document.ui.id, originalInput = reopened.document.ui.controls[0].id;
    library = duplicateDraft(library, id, reopened.name + " duplicate");
    const duplicate = library.drafts.at(-1);
    assert.notEqual(duplicate.document.ui.id, originalId);
    assert.notEqual(duplicate.document.ui.controls[0].id, originalInput);
    assert.notEqual(duplicate.assets[0].assetId, reopened.assets[0].assetId);
    for (const entry of [reopened, duplicate]) {
      const imported = await importBundle(await exportBundle(entry.document, entry.assets));
      assert.deepEqual(imported.document, entry.document); assert.deepEqual(imported.assets, entry.assets);
      assert.equal(imported.document.ui.pages.length, 2);
      assert.ok(imported.document.presentation.widgets.some(widget => widget.pageId === imported.document.ui.pages[1].id));
      assert.ok(imported.document.presentation.viewports.some(viewport => viewport.backgroundAssetId === imported.assets[0].assetId));
      const before = await publishDocument(entry.document, entry.assets), after = await publishDocument(imported.document, imported.assets);
      assert.equal(after.publicationHash, before.publicationHash); assert.equal(after.capabilityFingerprint, before.capabilityFingerprint);
    }
  }
  assert.equal(library.drafts.length, 4);
});

test("canonical publication round-trip ignores object key order while preserving logical page item order and values", async () => {
  for (const document of [flightTemplate, racingTemplate]) {
    const published=await publishDocument(document,[]);
    const reparsed=JSON.parse(canonical(published.document));
    assert.equal(validate(reparsed).ok,true);
    assert.equal((await publishDocument(reparsed,[])).publicationHash,published.publicationHash);
    const reversed=clone(reparsed);reversed.ui.pages[0].items.reverse();
    assert.equal(validate(reversed).ok,false);
    const altered=clone(reparsed);altered.ui.pages[0].items[0].bounds.x+=0.001;
    assert.equal(validate(altered).ok,false);
  }
});
