import type { Validation } from "../../companion-contracts/src/index.ts";
import { DESIGNER_LIMITS } from "./types.ts";
import type { DesignerLibrary, DraftEntry } from "./types.ts";
import { assert, checked, clone, exact, label, newId, normalizeToPortableUi, stableId } from "./model.ts";
import { validateAssets } from "./assets.ts";
import { resolveWidgetProperties } from "./widget-model.ts";
export function createLibrary(): DesignerLibrary { return { schemaVersion: 1, drafts: [] }; }
export async function saveDraft(library: DesignerLibrary, entry: DraftEntry): Promise<DesignerLibrary> {
  stableId(entry.id); label(entry.name); const document = checked(entry.document), assets = await validateAssets(document, entry.assets);
  const next = clone(library); const index = next.drafts.findIndex(draft => draft.id === entry.id);
  if (index < 0) { assert(next.drafts.length < DESIGNER_LIMITS.drafts, "Draft library is full"); next.drafts.push({ id: entry.id, name: entry.name, document, assets }); }
  else next.drafts[index] = { id: entry.id, name: entry.name, document, assets };
  return next;
}
export function openDraft(library: DesignerLibrary, id: string): DraftEntry {
  const draft = library.drafts.find(draft => draft.id === id); assert(draft, "Unknown draft"); return clone(draft);
}
export function deleteDraft(library: DesignerLibrary, id: string): DesignerLibrary {
  assert(library.drafts.some(draft => draft.id === id), "Unknown draft"); const next = clone(library); next.drafts = next.drafts.filter(draft => draft.id !== id); return next;
}
export function duplicateDraft(library: DesignerLibrary, id: string, name?: string): DesignerLibrary {
  assert(library.drafts.length < DESIGNER_LIMITS.drafts, "Draft library is full");
  const entry = openDraft(library, id); entry.id = newId("draft"); entry.name = name ?? entry.name + " copy"; label(entry.name);
  const doc = entry.document, ids = new Map<string, string>(); const remap = (id: string, prefix: string) => { if (!ids.has(id)) ids.set(id, newId(prefix)); return ids.get(id)!; };
  doc.ui.id = newId("ui");
  for (const control of doc.ui.controls) control.id = remap(control.id, "input");
  for (const output of doc.ui.feedbackWidgets) { output.id = remap(output.id, "output"); output.channelId = remap(output.channelId, "channel"); }
  for (const page of doc.ui.pages) page.id = remap(page.id, "page");
  // Definition assets are namespaced by the portable document. Keeping their
  // IDs preserves immutable recipe content while the copy gets new control IDs.
  if (doc.schemaVersion === 3) for (const asset of doc.ui.assets) ids.set(asset.id, asset.id);
  for (const asset of doc.ui.assets) asset.id = remap(asset.id, "asset");
  for (const target of doc.targets) target.id = remap(target.id, "target");
  for (const group of doc.presentation.groups) { group.id = remap(group.id, "group"); group.pageId = remap(group.pageId, "page"); }
  for (const viewport of doc.presentation.viewports) {
    viewport.pageId = remap(viewport.pageId, "page"); if (viewport.backgroundAssetId) viewport.backgroundAssetId = remap(viewport.backgroundAssetId, "asset");
    for (const region of viewport.regions ?? []) region.id = remap(region.id, "viewport");
  }
  for (const widget of doc.presentation.widgets) {
    if (widget.kind === "component") {
      const definition = doc.presentation.definitions!.find(d => d.id === widget.config.definitionId && d.version === widget.config.definitionVersion)!;
      if (definition.interaction.kind === "navigate") {
        const property = definition.interaction.pageProperty;
        const previous = resolveWidgetProperties(definition, widget.config.properties)[property];
        widget.config.properties[property] = remap(String(previous), "page");
      }
    }
    if(widget.kind === "image" && widget.config.assetId) widget.config.assetId = remap(widget.config.assetId, "asset");
    widget.id = newId("widget"); widget.pageId = remap(widget.pageId, "page"); if (widget.groupId) widget.groupId = remap(widget.groupId, "group");
    widget.controlIds = widget.controlIds.map(id => remap(id, "input")); widget.feedbackIds = widget.feedbackIds.map(id => remap(id, "output"));
    if ((widget.kind === "button" || widget.kind === "icon") && widget.config.iconAssetId) widget.config.iconAssetId = remap(widget.config.iconAssetId, "asset");
  }
  for (const binding of doc.bindings) { binding.controlId = remap(binding.controlId, "input"); binding.targetId = remap(binding.targetId, "target"); }
  for (const asset of entry.assets) asset.assetId = remap(asset.assetId, "asset");
  doc.ui = normalizeToPortableUi(doc); entry.document = checked(doc); return { schemaVersion: 1, drafts: [...clone(library.drafts), entry] };
}
export async function validateLibrary(input: unknown): Promise<Validation<DesignerLibrary>> {
  try {
    const value = exact(input, ["schemaVersion", "drafts"]); assert(value.schemaVersion === 1 && Array.isArray(value.drafts) && value.drafts.length <= DESIGNER_LIMITS.drafts, "Invalid library");
    let library = createLibrary(); const ids = new Set<string>();
    for (const item of value.drafts) {
      const entry = exact(item, ["id", "name", "document", "assets"]); stableId(entry.id); assert(!ids.has(entry.id), "Duplicate draft ID"); ids.add(entry.id);
      library = await saveDraft(library, entry as unknown as DraftEntry);
    }
    return { ok: true, value: library };
  } catch (error) { return { ok: false, error: error instanceof Error ? error.message : "Invalid library" }; }
}
