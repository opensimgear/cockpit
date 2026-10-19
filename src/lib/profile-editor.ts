import { diagramLayout } from './profile-diagram';
import { DEFAULT_PROFILE_ID, MAX_PROFILES, captureProfileSession, parseProfileDocument, profileAssignmentsError, profileDocumentError, type DeepReadonly, type ProfileDocument, type ProfileLocalAssignment, type ProfileResult, type ProfileSession, type ProfileSummary } from './profiles';
import type { ProfileInventoryClient, ResourceSummary } from './profile-inventory';
export const PROFILE_RESOURCE_CAPACITY = 64;
export const PROFILE_BLOCK_WIDTH = 288;
export const PROFILE_BLOCK_HEIGHT = 232;
export type EditorSaveResult = ProfileResult & {
    document: ProfileDocument;
    assignments: ProfileLocalAssignment[];
    profiles: ProfileSummary[];
};
export type ProfileEditorApi = {
    list(): Promise<ProfileResult>;
    getAssignments(id: string): Promise<ProfileResult>;
    editorSave(document: ProfileDocument, expectedRevision: number | null, assignments: ProfileLocalAssignment[]): Promise<EditorSaveResult>;
};
export type ProfileDraft = {
    document: ProfileDocument;
    assignments: ProfileLocalAssignment[];
    baseDocument: ProfileDocument;
    baseAssignments: ProfileLocalAssignment[];
    baseRevision: number | null;
};
export type ProfileWorkspaceState = {
    view: 'list' | 'editor';
    profiles: ProfileSummary[];
    session: DeepReadonly<ProfileSession> | null;
    editor: ProfileDraft | null;
    inventory: DeepReadonly<ResourceSummary>[];
    inventoryLoaded: boolean;
    busy: boolean;
    inventoryBusy: boolean;
    readOnly: boolean;
    diagnostics: string[];
    error: string;
    inventoryError: string;
};
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));
const uuid = (value: string) => /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value) && value !== DEFAULT_PROFILE_ID;
const freeze = <T>(value: T): DeepReadonly<T> => { if (value && typeof value === 'object' && !Object.isFrozen(value)) {
    Object.values(value).forEach(freeze);
    Object.freeze(value);
} return value as DeepReadonly<T>; };
export function profileResourceKind(resource: Pick<ResourceSummary, 'resourceType'>): 'device' | 'companion' | 'game' {
    return resource.resourceType === 'companion' ? 'companion' : resource.resourceType === 'game-integration' ? 'game' : 'device';
}
export function profileCategory(resource: Pick<ResourceSummary, 'resourceType'>): string {
    return { 'host-controller': 'Physical controller', 'osg-module': 'OSG peripheral', companion: 'Companion', 'game-integration': 'Game integration' }[resource.resourceType];
}
export function profileAutoLayout(ids: string[], bindings:readonly DeepReadonly<ProfileDocument['bindings'][number]>[]=[]): ProfileDocument['editor']['positions'] {
    if(bindings.length)return diagramLayout(ids,bindings,PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT);
    const columns = Math.max(1, Math.ceil(Math.sqrt(ids.length)));
    return ids.map((nodeId, index) => ({ nodeId, x: 48 + index % columns * (PROFILE_BLOCK_WIDTH + 48), y: 48 + Math.floor(index / columns) * (PROFILE_BLOCK_HEIGHT + 48) }));
}
export function blankProfile(id: string, name = 'New profile'): ProfileDocument {
    if (!uuid(id))
        throw new Error('A new profile requires a fresh canonical UUID.');
    const document: ProfileDocument = { schemaVersion: 2, id, name, revision: 1, eligibility: { gameIds: [] }, resources: [], bindings: [], processingNodes: [], virtualOutputs: [], visibility: [], overrides: [], companionAssignments: [], editor: { positions: [], viewport: { x: 0, y: 0, zoom: 1 } } };
    const error = profileDocumentError(document);
    if (error)
        throw new Error(error);
    return document;
}
const boundedName = (name: string) => [...name.trim()].slice(0, 80).join('') || 'Resource';
function freshResourceId(document: ProfileDocument): string {
    const used = new Set([...document.resources, ...document.processingNodes, ...document.virtualOutputs,...(document.desktopOutputs??[])].map(node => node.id));
    for (let n = 1; n <= 400; n++)
        if (!used.has(`resource-${n}`))
            return `resource-${n}`;
    throw new Error('No resource identity is available.');
}
/** Native identities stay solely in machine assignments, never in the portable document. */
export function addProfileResources(document: ProfileDocument, assignments: ProfileLocalAssignment[], resources: readonly DeepReadonly<ResourceSummary>[]): {
    document: ProfileDocument;
    assignments: ProfileLocalAssignment[];
} {
    const known = new Set(assignments.map(assignment => assignment.sourceId));
    const incoming = resources.filter(resource => !known.has(resource.id)).sort((a, b) => a.resourceType === b.resourceType ? (a.id < b.id ? -1 : a.id > b.id ? 1 : 0) : a.resourceType < b.resourceType ? -1 : 1);
    if (new Set(incoming.map(resource => resource.id)).size !== incoming.length)
        throw new Error('Inventory contains duplicate resource identities. Refresh before adding resources.');
    if (document.resources.length + incoming.length > PROFILE_RESOURCE_CAPACITY)
        throw new Error(`This inventory needs ${document.resources.length + incoming.length} resource blocks; a profile supports ${PROFILE_RESOURCE_CAPACITY}. Add a smaller explicit selection from inventory. No resources were omitted or added.`);
    const next = clone(document), local = clone(assignments);
    for (const resource of incoming) {
        const id = freshResourceId(next);
        next.resources.push({ id, name: boundedName(resource.name), kind: profileResourceKind(resource), required: false, ports: [] });
        local.push({ resourceId: id, sourceId: resource.id, ports: [] });
    }
    const placed = new Set(next.editor.positions.map(position => position.nodeId));
    const missing = [...next.resources, ...next.processingNodes, ...next.virtualOutputs,...(next.desktopOutputs??[])].map(node => node.id).filter(id => !placed.has(id));
    const bottom = next.editor.positions.reduce((value, position) => Math.max(value, position.y + PROFILE_BLOCK_HEIGHT + 48), 0);
    next.editor.positions.push(...profileAutoLayout(missing).map(position => ({ ...position, y: position.y + bottom })));
    const error = profileDocumentError(next) || profileAssignmentsError(local);
    if (error)
        throw new Error(error);
    return { document: next, assignments: local };
}
export function seedProfile(id: string, resources: readonly DeepReadonly<ResourceSummary>[], name = 'New profile'): {
    document: ProfileDocument;
    assignments: ProfileLocalAssignment[];
} {
    if (resources.length > PROFILE_RESOURCE_CAPACITY)
        throw new Error(`Inventory contains ${resources.length} resources; a profile supports ${PROFILE_RESOURCE_CAPACITY}. Start with an explicit selection of at most ${PROFILE_RESOURCE_CAPACITY} resources. Nothing was truncated or saved.`);
    return addProfileResources(blankProfile(id, name), [], resources);
}
export function copyProfile(document: ProfileDocument, id: string): ProfileDocument {
    const next = clone(document);
    if (!uuid(id) || id === document.id)
        throw new Error('A copy requires a new independent canonical UUID.');
    next.id = id;
    next.revision = 1;
    next.name = `${[...document.name].slice(0, 75).join('')} copy`;
    return parseProfileDocument(JSON.stringify(next));
}
export function profileEditorDirty(editor: DeepReadonly<ProfileDraft>): boolean {
    return editor.baseRevision === null || JSON.stringify(editor.document) !== JSON.stringify(editor.baseDocument) || JSON.stringify(editor.assignments) !== JSON.stringify(editor.baseAssignments);
}
function draft(document: ProfileDocument, assignments: ProfileLocalAssignment[], baseRevision: number | null): ProfileDraft {
    const checked = parseProfileDocument(JSON.stringify(document));
    const error = profileAssignmentsError(assignments);
    if (error)
        throw new Error(error);
    return { document: checked, assignments: clone(assignments), baseDocument: clone(checked), baseAssignments: clone(assignments), baseRevision };
}
/** Local authoring state. Its API deliberately has no select/apply/start methods. */
export class ProfileEditorStore {
    private value: ProfileWorkspaceState = { view: 'list', profiles: [], session: null, editor: null, inventory: [], inventoryLoaded: false, busy: false, inventoryBusy: false, readOnly: false, diagnostics: [], error: '', inventoryError: '' };
    private listeners = new Set<(state: DeepReadonly<ProfileWorkspaceState>) => void>();
    private epoch = 0;
    private inventoryEpoch = 0;
    constructor(private api: ProfileEditorApi, private inventory: Pick<ProfileInventoryClient, 'listAll'>, private newId: () => string = () => globalThis.crypto.randomUUID()) { }
    get state(): DeepReadonly<ProfileWorkspaceState> { return freeze(this.value); }
    get editorEpoch(): number { return this.epoch; }
    get inventoryRevision(): number { return this.inventoryEpoch; }
    subscribe(listener: (state: DeepReadonly<ProfileWorkspaceState>) => void): () => void { this.listeners.add(listener); listener(this.state); return () => { this.listeners.delete(listener); }; }
    private set(patch: Partial<ProfileWorkspaceState>) { this.value = { ...this.value, ...patch }; for (const listener of this.listeners)
        listener(this.state); }
    private session(result: ProfileResult) { return result.session ? captureProfileSession(result.session) : this.value.session; }
    async refresh(): Promise<void> { const epoch = ++this.epoch; this.set({ busy: true, error: '' }); try {
        const result = await this.api.list();
        if (epoch !== this.epoch)
            return;
        if (!result.profiles)
            throw new Error('The profile list response is missing its profiles.');
        this.set({ profiles: clone(result.profiles), session: this.session(result), diagnostics: clone(result.diagnostics ?? []), readOnly: result.readOnly === true });
    }
    catch (error) {
        if (epoch === this.epoch)
            this.set({ error: String(error instanceof Error ? error.message : error) });
    }
    finally {
        if (epoch === this.epoch)
            this.set({ busy: false });
    } }
    async refreshInventory(): Promise<void> { const epoch = ++this.inventoryEpoch; this.set({ inventoryBusy: true, inventoryError: '' }); try {
        const result = await this.inventory.listAll();
        if (epoch !== this.inventoryEpoch)
            return;
        this.set({ inventory: clone(result.resources) as ResourceSummary[], inventoryLoaded: true, inventoryError: result.inventoryDiagnostics?.join(' ') ?? '' });
    }
    catch (error) {
        if (epoch === this.inventoryEpoch)
            this.set({ inventoryLoaded: false, inventoryError: error instanceof Error ? error.message : String(error) });
    }
    finally {
        if (epoch === this.inventoryEpoch)
            this.set({ inventoryBusy: false });
    } }
    async open(id: string): Promise<void> { const epoch = ++this.epoch; this.set({ busy: true, error: '' }); try {
        const result = await this.api.getAssignments(id);
        if (epoch !== this.epoch)
            return;
        if (!result.document || result.document.id !== id || !result.assignments)
            throw new Error('The requested profile and its local assignments were not returned together.');
        this.set({ view: 'editor', editor: draft(result.document, result.assignments, result.document.revision), session: this.session(result) });
    }
    catch (error) {
        if (epoch === this.epoch)
            this.set({ error: error instanceof Error ? error.message : String(error) });
    }
    finally {
        if (epoch === this.epoch)
            this.set({ busy: false });
    } }
    beginBlank(resources: readonly DeepReadonly<ResourceSummary>[] = this.value.inventory): void { this.authoringAllowed(); if (!this.value.inventoryLoaded)
        throw new Error('Refresh resource inventory before creating a seeded profile.'); if (this.value.profiles.length >= MAX_PROFILES)
        throw new Error('The profile library is full (32 including Default). Remove an unused named profile first.'); const seeded = seedProfile(this.newId(), resources); this.epoch++; this.set({ view: 'editor', editor: draft(seeded.document, seeded.assignments, null), busy: false, error: '' }); }
    async beginCopy(id: string): Promise<void> { this.authoringAllowed(); if (this.value.profiles.length >= MAX_PROFILES)
        throw new Error('The profile library is full (32 including Default). Remove an unused named profile first.'); const epoch = ++this.epoch; this.set({ busy: true, error: '' }); try {
        const result = await this.api.getAssignments(id);
        if (epoch !== this.epoch)
            return;
        if (!result.document || result.document.id !== id)
            throw new Error('The requested copy source was not returned.');
        const document = copyProfile(result.document, this.newId());
        this.set({ view: 'editor', editor: draft(document, [], null), session: this.session(result) });
    }
    catch (error) {
        if (epoch === this.epoch)
            this.set({ error: error instanceof Error ? error.message : String(error) });
    }
    finally {
        if (epoch === this.epoch)
            this.set({ busy: false });
    } }
    private authoringAllowed() { if (this.value.readOnly)
        throw new Error('The profile library is read-only. Resolve its diagnostics before saving changes.'); }
    update(document: ProfileDocument, assignments?: ProfileLocalAssignment[]): void { this.authoringAllowed(); const editor = this.value.editor; if (!editor || document.id !== editor.document.id || document.revision !== editor.document.revision)
        throw new Error('Keep the current editor identity and revision.'); const error = profileDocumentError({ ...document, name: 'Profile draft' }) || profileAssignmentsError(assignments ?? editor.assignments); if (error)
        throw new Error(error); this.set({ editor: { ...editor, document: clone(document), assignments: clone(assignments ?? editor.assignments) }, error: '' }); }
    addResources(resources: readonly DeepReadonly<ResourceSummary>[]): void { const editor = this.value.editor; if (!editor)
        throw new Error('Open a profile first.'); const next = addProfileResources(editor.document, editor.assignments, resources); this.update(next.document, next.assignments); }
    back(discard = false): boolean { if (this.value.editor && profileEditorDirty(this.value.editor) && !discard)
        return false; this.epoch++; this.set({ view: 'list', editor: null, busy: false, error: '' }); return true; }
    async save(): Promise<void> { this.authoringAllowed(); const editor = this.value.editor; if (!editor || this.value.busy)
        return; const epoch = this.epoch; const submitted = clone(editor); this.set({ busy: true, error: '' }); try {
        const error = profileDocumentError(submitted.document) || profileAssignmentsError(submitted.assignments);
        if (error)
            throw new Error(error);
        const result = await this.api.editorSave(submitted.document, submitted.baseRevision, submitted.assignments);
        if (!result.document || !result.assignments || result.document.id !== submitted.document.id)
            throw new Error('Save did not return this profile and its local assignments.');
        const summaries = result.profiles?.filter(row => row.id === result.document.id) ?? [];
        const summary = summaries[0];
        if (summaries.length !== 1 || summary.name !== result.document.name || summary.revision !== result.document.revision || summary.isDefault !== (result.document.id === DEFAULT_PROFILE_ID) || !/^sha256:[0-9a-f]{64}$/.test(summary.semanticHash))
            throw new Error('Save did not return the exact committed profile summary. Refresh the saved profile before saving again.');
        const committed = draft(result.document, result.assignments, result.document.revision);
        if (epoch !== this.epoch)
            return;
        const current = this.value.editor;
        if (!current || current.document.id !== submitted.document.id || current.baseRevision !== submitted.baseRevision)
            return;
        const unchanged = JSON.stringify(current.document) === JSON.stringify(submitted.document) && JSON.stringify(current.assignments) === JSON.stringify(submitted.assignments);
        const next = unchanged ? committed : { ...current, document: { ...clone(current.document), revision: result.document.revision }, baseRevision: result.document.revision, baseDocument: clone(result.document), baseAssignments: clone(result.assignments) };
        const rows = this.value.profiles.filter(row => row.id !== result.document.id);
        rows.push(clone(summary));
        this.set({ editor: next, profiles: rows, session: this.session(result) });
    }
    catch (error) {
        if (epoch === this.epoch)
            this.set({ error: error instanceof Error ? error.message : String(error) });
    }
    finally {
        if (epoch === this.epoch)
            this.set({ busy: false });
    } }
    dispose(): void { this.epoch++; this.inventoryEpoch++; this.listeners.clear(); }
}

/** Routes are UI continuations, never runtime selection or activation requests. */
export class ProfileNavigationGuard {
    private pending: { continueNavigation: () => void; editorEpoch: number } | null = null;
    constructor(private store: ProfileEditorStore) {}
    get awaitingChoice(): boolean { return this.pending !== null; }
    request(continueNavigation: () => void = () => {}): 'left' | 'confirm' | 'busy' {
        if (this.store.state.busy) return 'busy';
        this.pending = { continueNavigation, editorEpoch: this.store.editorEpoch };
        if (!this.store.back()) return 'confirm';
        this.complete();
        return 'left';
    }
    keepEditing(): void { this.pending = null; }
    discard(): boolean {
        if (this.store.state.busy) return false;
        this.store.back(true);
        this.complete();
        return true;
    }
    async save(): Promise<boolean> {
        const pending = this.pending;
        if (!pending || this.store.state.busy || this.store.state.readOnly) return false;
        await this.store.save();
        if (this.pending !== pending) return false;
        if (this.store.editorEpoch !== pending.editorEpoch) { this.pending = null; return false; }
        const current = this.store.state;
        if (current.error || current.editor && profileEditorDirty(current.editor)) return false;
        if (!this.store.back()) return false;
        this.complete();
        return true;
    }
    private complete(): void {
        const next = this.pending;
        this.pending = null;
        next?.continueNavigation();
    }
}
