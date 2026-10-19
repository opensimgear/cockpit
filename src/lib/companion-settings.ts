import type { Publication } from '../../packages/companion-designer/src/index.ts';
import { sendPublication, stableCompanionId, type CompanionSend } from './companions.ts';
import { companionScreenSize, panelScreenReason } from './companion-screen.ts';

export interface CompanionSettingsInput {
  companionId: string;
  name: string;
  panelHash: string;
  publications: Publication[];
  saveName?: boolean;
  savePanel?: boolean;
}

export interface CompanionSettingsSaveState {
  companionId: string;
  saving: boolean;
  pending: boolean;
  error: string;
}

function capture(input: CompanionSettingsInput): CompanionSettingsInput {
  return { companionId: input.companionId, name: input.name, panelHash: input.panelHash, publications: [...input.publications], saveName: input.saveName, savePanel: input.savePanel };
}

/** Save only changed settings for the captured installation; a rename never touches its active panel. */
export async function saveCompanionSettings(input: CompanionSettingsInput, send: CompanionSend): Promise<void> {
  const { companionId: requestedId, name: requestedName, panelHash, publications, saveName = true, savePanel = true } = capture(input);
  const companionId = stableCompanionId(requestedId);
  const name = requestedName.trim();
  if (saveName && (!name || name.length > 80 || /[\u0000-\u001f]/.test(name))) throw new Error('Enter a companion name of 1 to 80 characters.');

  const state = await send('companion_status');
  const client = state.host?.clients?.find(client => client.companionId === companionId);
  if (!client) throw new Error('This companion is no longer paired with Cockpit.');
  const instances = (state.host?.instances ?? []).filter(instance => instance.companionId === companionId);
  const currentHash = String(instances[0]?.publicationHash ?? '');
  const panelChanged = savePanel && panelHash !== currentHash;
  if (panelChanged && instances.length > 1) throw new Error('This companion has multiple assignments. Edit assignments in Panels.');
  const panel = publications.find(publication => publication.publicationHash === panelHash);
  if (panelChanged && panelHash && !panel) throw new Error('Choose a saved panel.');
  if(panelChanged&&panel&&client.kind==='esp32'){
    const reason=panelScreenReason(panel,companionScreenSize(state,companionId),state.firmware?.[companionId]?.version??'unknown');
    if(reason)throw new Error(reason);
  }

  if (panelChanged && panel) await sendPublication(panel, send);
  if (saveName && client.name !== name) await send('companion_update', { companionId, name });
  if (panelChanged && !panelHash && instances[0]) {
    await send('companion_unassign', { instanceId: stableCompanionId(instances[0].instanceId) });
  } else if (panelChanged && panel) {
    await send('companion_assign', { companionId, instanceId: instances[0] ? stableCompanionId(instances[0].instanceId) : crypto.randomUUID(), publicationHash: panelHash });
  }
}

/** One serialized queue survives editor navigation and retains failures until an explicit retry or edit. */
export function createCompanionSettingsAutosave(send: CompanionSend, onChange?: (state: CompanionSettingsSaveState) => void) {
  type Entry = { input: CompanionSettingsInput; due: number; failed: boolean };
  const pending = new Map<string, Entry>();
  const errors = new Map<string, string>();
  let activeId = '';
  let running: Promise<void> | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let flushing = 0;

  function emit(companionId: string) {
    const state = { companionId, saving: activeId === companionId, pending: pending.has(companionId), error: errors.get(companionId) ?? '' };
    // View callbacks must not interrupt a captured native write after navigation.
    try { onChange?.(state); } catch { /* Persistence remains independent of the mounted view. */ }
  }

  function stopTimer() {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  function nextReady(): Entry | undefined {
    for (const [id, entry] of pending) {
      if (!entry.failed && entry.due <= Date.now()) { pending.delete(id); return entry; }
    }
    return undefined;
  }

  function arm() {
    stopTimer();
    if (running) return;
    const due = Math.min(...[...pending.values()].filter(entry => !entry.failed).map(entry => entry.due));
    if (!Number.isFinite(due)) return;
    timer = setTimeout(() => { timer = undefined; void start(); }, Math.max(0, due - Date.now()));
  }

  function start(): Promise<void> | undefined {
    if (running) return running;
    const first = nextReady();
    if (!first) { arm(); return; }
    stopTimer();
    running = run(first).finally(() => { running = undefined; arm(); });
    return running;
  }

  async function run(first: Entry) {
    let entry: Entry | undefined = first;
    while (entry) {
      const current = entry;
      const id = current.input.companionId;
      activeId = id;
      errors.delete(id);
      emit(id);
      try { await saveCompanionSettings(current.input, send); }
      catch (error) {
        errors.set(id, error instanceof Error ? error.message : String(error));
        // A newer edit replaces this failed snapshot rather than being overwritten by it.
        if (!pending.has(id)) pending.set(id, { ...current, failed: true });
      } finally {
        activeId = '';
        emit(id);
      }
      entry = nextReady();
    }
  }

  function schedule(input: CompanionSettingsInput, delayMs = 400) {
    const snapshot = capture(input);
    pending.set(snapshot.companionId, { input: snapshot, due: Date.now() + (flushing ? 0 : Math.max(0, delayMs)), failed: false });
    errors.delete(snapshot.companionId);
    emit(snapshot.companionId);
    arm();
  }

  async function flush(): Promise<void> {
    flushing++;
    stopTimer();
    for (const [id, entry] of pending) {
      entry.due = Date.now(); entry.failed = false; errors.delete(id); emit(id);
    }
    try {
      let work: Promise<void> | undefined;
      while ((work = start())) await work;
    } finally { flushing--; arm(); }
  }

  function discard(companionId: string) {
    pending.delete(companionId);
    errors.delete(companionId);
    emit(companionId);
    arm();
  }

  return { schedule, flush, discard };
}
