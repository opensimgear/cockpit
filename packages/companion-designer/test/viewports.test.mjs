import test from "node:test";
import assert from "node:assert/strict";
import {
  newDocument, createDesignerHistory, applyDesignerCommand as apply, undoDesigner, redoDesigner,
  viewportRegions, widgetViewport, boundsContain, boundsOverlap, normalizeToPortableUi,
  validateDesignerDocument as validate, flightTemplate, publishDocument, compareDesignerPublications,
  createLibrary, saveDraft, duplicateDraft, exportBundle, importBundle, DESIGNER_LIMITS,
} from "../src/index.ts";

const history = () => createDesignerHistory(newDocument("Multi viewport screen"));
const first = doc => doc.presentation.widgets[0];
const page = h => h.present.ui.pages[0].id;
const split = h => apply(h, { type: "add-viewport", pageId: page(h) });

test("legacy screens expose a detached full-screen viewport without changing persisted documents or hashes", async () => {
  const document = structuredClone(flightTemplate), before = structuredClone(document);
  const publication = await publishDocument(document, []), regions = viewportRegions(document, document.ui.pages[0].id);
  assert.equal(regions.length, 1);
  assert.equal(regions[0].id, "viewport-" + document.ui.pages[0].id.slice(0, 55));
  assert.deepEqual(regions[0].bounds, { x: 0, y: 0, width: 1, height: 1 });
  regions[0].bounds.width = 0.5;
  assert.deepEqual(document, before);
  assert.deepEqual(validate(document).value, before);
  assert.equal((await publishDocument(document, [])).publicationHash, publication.publicationHash);
  assert.throws(() => viewportRegions(document, "missing"), /Unknown/);
});

test("implicit viewports choose deterministic available IDs before their first edit or split", async () => {
  const document = newDocument("Existing viewport-like IDs"), oldPageId = document.ui.pages[0].id;
  document.ui.pages[0].id = "page";
  document.presentation.viewports[0].pageId = "page";
  for (const widget of document.presentation.widgets) if (widget.pageId === oldPageId) widget.pageId = "page";
  first(document).id = "viewport-page";
  document.ui.id = "viewport-page-2";
  document.targets[0].id = "viewport-page-3";
  for (const binding of document.bindings) binding.targetId = document.targets[0].id;
  document.ui = normalizeToPortableUi(document);
  const before = structuredClone(document), publication = await publishDocument(document, []);
  assert.equal(validate(document).ok, true);
  const region = viewportRegions(document, "page")[0];
  assert.equal(region.id, "viewport-page-4");
  assert.deepEqual(viewportRegions(document, "page")[0], region);
  assert.deepEqual(document, before);
  assert.equal((await publishDocument(document, [])).publicationHash, publication.publicationHash);
  const h = createDesignerHistory(document);
  const renamed = apply(h, { type: "update-viewport", pageId: "page", id: region.id, patch: { label: "Existing screen" } });
  const divided = apply(h, { type: "add-viewport", pageId: "page", sourceId: region.id });
  assert.equal(validate(renamed.present).ok, true);
  assert.equal(validate(divided.present).ok, true);
  assert.equal(renamed.present.presentation.viewports[0].regions[0].id, region.id);
  assert.equal(divided.present.presentation.viewports[0].regions[0].id, region.id);
});

test("long legacy page IDs with the same prefix materialize distinct viewport IDs within 64 characters", () => {
  let h = history(); h = apply(h, { type: "add-page", title: "Another companion page" });
  const document = h.present, commonPrefix = "p".repeat(55), pageIds = [commonPrefix + "a".repeat(9), commonPrefix + "b".repeat(9)];
  for (let i = 0; i < pageIds.length; i++) {
    const oldId = document.ui.pages[i].id;
    document.ui.pages[i].id = pageIds[i]; document.presentation.viewports[i].pageId = pageIds[i];
    for (const widget of document.presentation.widgets) if (widget.pageId === oldId) widget.pageId = pageIds[i];
  }
  const base = "viewport-" + commonPrefix;
  first(document).id = base;
  document.ui = normalizeToPortableUi(document);
  h = createDesignerHistory(document);
  const firstId = viewportRegions(h.present, pageIds[0])[0].id;
  assert.equal(firstId, base.slice(0, 62) + "-2");
  h = apply(h, { type: "update-viewport", pageId: pageIds[0], id: firstId, patch: { label: "First area" } });
  const secondId = viewportRegions(h.present, pageIds[1])[0].id;
  assert.equal(secondId, base.slice(0, 62) + "-3");
  h = apply(h, { type: "update-viewport", pageId: pageIds[1], id: secondId, patch: { label: "Second area" } });
  assert.equal(validate(h.present).ok, true);
  assert.equal(firstId.length, 64); assert.equal(secondId.length, 64);
  assert.notEqual(firstId, secondId);
});

test("adding a viewport splits a full screen and scales its widgets in one undoable operation", () => {
  let h = history(); const original = structuredClone(h.present), widget = first(original);
  h = split(h); const [left, right] = viewportRegions(h.present, page(h));
  assert.deepEqual(left.bounds, { x: 0, y: 0, width: 0.5, height: 1 });
  assert.deepEqual(right.bounds, { x: 0.5, y: 0, width: 0.5, height: 1 });
  assert.deepEqual(first(h.present).bounds, { x: widget.bounds.x / 2, y: widget.bounds.y, width: widget.bounds.width / 2, height: widget.bounds.height });
  assert.deepEqual(first(h.present).controlIds, widget.controlIds);
  assert.equal(widgetViewport(h.present, first(h.present)).id, left.id);
  assert.deepEqual(h.present.ui, normalizeToPortableUi(h.present));
  assert.deepEqual(undoDesigner(h).present, original);
  assert.deepEqual(redoDesigner(undoDesigner(h)).present, h.present);
  h = apply(h, { type: "add-widget", pageId: page(h), viewportId: right.id, kind: "gauge" });
  assert.equal(widgetViewport(h.present, h.present.presentation.widgets.at(-1)).id, right.id);
  assert.equal(validate(h.present).ok, true);
});

test("moving and resizing a viewport keeps widget positions proportional and preserves screen settings", () => {
  let h = history(), pageId = page(h);
  h = apply(h, { type: "set-viewport", pageId, width: 1920, height: 1080, safeInsets: { top: 0.05, left: 0.05, right: 0, bottom: 0 } });
  const legacyBounds = structuredClone(first(h.present).bounds);
  h = split(h); const region = viewportRegions(h.present, pageId)[0], original = structuredClone(h.present);
  h = apply(h, { type: "update-viewport", pageId, id: region.id, patch: { label: "Left instruments", bounds: { x: 0.1, y: 0.2, width: 0.3, height: 0.6 } } });
  assert.equal(first(h.present).bounds.x, 0.1 + legacyBounds.x * 0.3);
  assert.equal(first(h.present).bounds.y, 0.2 + legacyBounds.y * 0.6);
  assert.equal(first(h.present).bounds.width, legacyBounds.width * 0.3);
  assert.equal(first(h.present).bounds.height, legacyBounds.height * 0.6);
  const regions = structuredClone(h.present.presentation.viewports[0].regions);
  h = apply(h, { type: "set-viewport", pageId, width: 2560, height: 1440 });
  assert.deepEqual(h.present.presentation.viewports[0].regions, regions);
  assert.equal(h.present.presentation.viewports[0].safeInsets.left, 0.05);
  assert.deepEqual(undoDesigner(undoDesigner(h)).present, original);
});

test("new viewports fill empty screen space before splitting the selected viewport", () => {
  let h = history(), pageId = page(h), initial = viewportRegions(h.present, pageId)[0];
  h = apply(h, { type: "update-viewport", pageId, id: initial.id, patch: { bounds: { x: 0, y: 0, width: 0.5, height: 0.5 } } });
  const widgetBounds = structuredClone(first(h.present).bounds);
  h = split(h); h = split(h);
  const regions = viewportRegions(h.present, pageId);
  assert.equal(regions.reduce((sum, region) => sum + region.bounds.width * region.bounds.height, 0), 1);
  assert.deepEqual(first(h.present).bounds, widgetBounds);
  h = apply(h, { type: "add-viewport", pageId, sourceId: initial.id });
  assert.equal(viewportRegions(h.present, pageId).length, 4);
  assert.ok(first(h.present).bounds.width < widgetBounds.width);
  assert.equal(validate(h.present).ok, true);
});

test("viewports can touch, while overlap, out-of-screen bounds, missing IDs and orphan widgets fail atomically", () => {
  const h = split(history()), pageId = page(h), [left, right] = viewportRegions(h.present, pageId), before = structuredClone(h);
  assert.equal(boundsOverlap(left.bounds, right.bounds), false);
  assert.equal(boundsContain(left.bounds, first(h.present).bounds), true);
  for (const [command, message] of [
    [{ type: "update-viewport", pageId, id: left.id, patch: { bounds: { x: 0, y: 0, width: 0.6, height: 1 } } }, /overlap/],
    [{ type: "update-viewport", pageId, id: right.id, patch: { bounds: { x: 0.7, y: 0, width: 0.5, height: 1 } } }, /inside the companion screen/],
    [{ type: "update-viewport", pageId, id: "missing", patch: { label: "Missing" } }, /Unknown viewport/],
    [{ type: "update-viewport", pageId, id: left.id, patch: { id: "changed" } }, /IDs cannot/],
    [{ type: "add-viewport", pageId, sourceId: "missing" }, /Unknown viewport/],
    [{ type: "add-widget", pageId, viewportId: "missing", kind: "button" }, /Unknown viewport/],
    [{ type: "add-widget", pageId, viewportId: right.id, kind: "button", bounds: { x: 0.4, y: 0.2, width: 0.2, height: 0.2 } }, /selected viewport/],
    [{ type: "move-widget", id: first(h.present).id, bounds: { x: 0.4, y: 0.2, width: 0.2, height: 0.2 } }, /entirely inside one viewport/],
    [{ type: "remove-viewport", pageId, id: left.id }, /contains widgets/],
  ]) {
    assert.throws(() => apply(h, command), message);
    assert.deepEqual(h, before);
  }
  const removed = apply(h, { type: "remove-viewport", pageId, id: right.id });
  assert.equal(viewportRegions(removed.present, pageId).length, 1);
  assert.throws(() => apply(removed, { type: "remove-viewport", pageId, id: left.id }), /at least one/);
  for (const mutate of [
    doc => delete doc.presentation.viewports[0].regions[0].id,
    doc => doc.presentation.viewports[0].regions[1].id = left.id,
    doc => doc.presentation.viewports[0].regions = [],
    doc => doc.presentation.viewports[0].regions[1].bounds.x = 0.4,
    doc => doc.presentation.viewports[0].regions[1].bounds.height = 1.1,
  ]) {
    const document = structuredClone(h.present); mutate(document); assert.equal(validate(document).ok, false);
  }
});

test("widget duplication and grid snapping keep widgets inside their viewport", () => {
  let h = split(history()), pageId = page(h), right = viewportRegions(h.present, pageId)[1];
  h = apply(h, { type: "add-widget", pageId, viewportId: right.id, kind: "text", bounds: { x: 0.8, y: 0.8, width: 0.2, height: 0.2 } });
  const original = h.present.presentation.widgets.at(-1);
  h = apply(h, { type: "duplicate-widget", id: original.id });
  assert.equal(widgetViewport(h.present, h.present.presentation.widgets.at(-1)).id, right.id);
  h = apply(h, { type: "update-viewport", pageId, id: right.id, patch: { bounds: { x: 0.53, y: 0, width: 0.47, height: 1 } } });
  h = apply(h, { type: "set-grid", columns: 2, rows: 2, snap: true });
  h = apply(h, { type: "snap", ids: [original.id] });
  assert.equal(widgetViewport(h.present, h.present.presentation.widgets.find(widget => widget.id === original.id)).id, right.id);
});

test("viewport metadata round-trips through bundles, publication and draft duplication", async () => {
  let h = split(history()), pageId = page(h), right = viewportRegions(h.present, pageId)[1];
  h = apply(h, { type: "add-widget", pageId, viewportId: right.id, kind: "text" });
  const document = h.present, publication = await publishDocument(document, []);
  const bundle = await importBundle(await exportBundle(document, []));
  assert.deepEqual(bundle.document, document);
  assert.equal((await publishDocument(bundle.document, [])).publicationHash, publication.publicationHash);
  const labelOnly = apply(h, { type: "update-viewport", pageId, id: right.id, patch: { label: "Navigation" } }).present;
  assert.equal(compareDesignerPublications(document, labelOnly), "styling");
  const library = await saveDraft(createLibrary(), { id: "multi", name: "Multiple areas", document, assets: [] });
  const copy = duplicateDraft(library, "multi").drafts[1].document;
  assert.equal(validate(copy).ok, true);
  assert.notEqual(copy.presentation.viewports[0].regions[0].id, document.presentation.viewports[0].regions[0].id);
  assert.deepEqual(copy.presentation.viewports[0].regions.map(region => region.bounds), document.presentation.viewports[0].regions.map(region => region.bounds));
});

test("viewport regions are bounded independently per page and page deletion cleans their contents", () => {
  let h = split(history()); h = apply(h, { type: "add-page", title: "Second screen" });
  const secondPage = h.present.ui.pages[1].id;
  h = apply(h, { type: "add-viewport", pageId: secondPage });
  const region = viewportRegions(h.present, secondPage)[1];
  h = apply(h, { type: "add-widget", pageId: secondPage, viewportId: region.id, kind: "button" });
  h = apply(h, { type: "delete-page", id: secondPage });
  assert.equal(h.present.presentation.viewports.length, 1);
  assert.equal(h.present.presentation.widgets.length, 1);
  let maximum = history();
  for (let i = 1; i < DESIGNER_LIMITS.viewportRegions; i++) maximum = split(maximum);
  assert.throws(() => split(maximum), /at most 32/);
});
