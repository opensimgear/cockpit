/** Revisioned desktop preferences. Browser previews use session memory only. */
export const UI_KEYS = ['theme', 'devices-mode', 'canvas-navigation', 'canvas-styling', 'remembered', 'tray', 'progress'] as const;
export type UiKey = typeof UI_KEYS[number];
export type UiValues = {
  theme: boolean;
  'devices-mode': 'list' | 'topology';
  'canvas-navigation': { wheelAction: 'pan' | 'zoom' };
  'canvas-styling': { arrowStyle: 'smooth' | 'orthogonal'; cornerRadius: number; collapseParallelWires: boolean };
  remembered: { components: unknown[]; forgotten: Record<string, unknown> } | null;
  tray: { closeToTray: boolean; minimizeToTray: boolean };
  progress: { delivery: 'system' | 'window'; position: 'bottom-right' | 'bottom-left' | 'top-right' | 'top-left' };
};
export type UiEntry<K extends UiKey = UiKey> = { revision: number; value: UiValues[K]; ready: boolean; busy: boolean; error: string };
export type UiSnapshot = { entries: Record<UiKey, { revision: number; value: unknown }> };
export type UiTransport = {
  read(): Promise<UiSnapshot>;
  write(key: UiKey, expectedRevision: number, value: unknown): Promise<{ revision: number; value: unknown }>;
};
const defaults: UiValues = {
  theme: false, 'devices-mode': 'list', 'canvas-navigation': { wheelAction: 'pan' },
  'canvas-styling': { arrowStyle: 'smooth', cornerRadius: 24, collapseParallelWires: true }, remembered: null,
  tray: { closeToTray: false, minimizeToTray: false }, progress: { delivery: 'system', position: 'bottom-right' },
};
const revision = (value: unknown): value is number => Number.isSafeInteger(value) && Number(value) >= 0;
const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
function exact(value: unknown, keys: string[]): value is Record<string, unknown> {
  return object(value) && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
}
export function validateUiValue<K extends UiKey>(key: K, value: unknown): UiValues[K] {
  let valid = false;
  switch (key) {
    case 'theme': valid = typeof value === 'boolean'; break;
    case 'devices-mode': valid = value === 'list' || value === 'topology'; break;
    case 'canvas-navigation': valid = exact(value, ['wheelAction']) && ['pan', 'zoom'].includes(String(value.wheelAction)); break;
    case 'canvas-styling': valid = exact(value, ['arrowStyle', 'cornerRadius', 'collapseParallelWires']) && ['smooth', 'orthogonal'].includes(String(value.arrowStyle)) && typeof value.cornerRadius === 'number' && Number.isFinite(value.cornerRadius) && value.cornerRadius >= 0 && value.cornerRadius <= 100 && typeof value.collapseParallelWires === 'boolean'; break;
    case 'remembered': valid = value === null || exact(value, ['components', 'forgotten']) && Array.isArray(value.components) && value.components.length <= 1024 && object(value.forgotten) && Object.keys(value.forgotten).length <= 4096; break;
    case 'tray': valid = exact(value, ['closeToTray', 'minimizeToTray']) && typeof value.closeToTray === 'boolean' && typeof value.minimizeToTray === 'boolean'; break;
    case 'progress': valid = exact(value, ['delivery', 'position']) && ['system', 'window'].includes(String(value.delivery)) && ['bottom-right', 'bottom-left', 'top-right', 'top-left'].includes(String(value.position)); break;
  }
  if (!valid) throw Error(`Saved ${key} preferences are invalid. Preserve the SQLite record and refresh.`);
  return structuredClone(value) as UiValues[K];
}
export class UiPreferences {
  private transport: UiTransport | null;
  private entries = new Map<UiKey, UiEntry>();
  private listeners = new Map<UiKey, Set<(entry: UiEntry) => void>>();
  private reading: Promise<boolean> | null = null;
  constructor(transport: UiTransport | null = null) {
    this.transport = transport;
    for (const key of UI_KEYS) this.entries.set(key, { revision: 0, value: structuredClone(defaults[key]), ready: !transport, busy: false, error: '' });
  }
  connect(transport: UiTransport) {
    if (this.transport || UI_KEYS.some(key => this.entries.get(key)!.revision > 0)) throw Error('UI preference storage is already initialized.');
    this.transport = transport;
    for (const key of UI_KEYS) this.update(key, { ready: false });
  }
  get<K extends UiKey>(key: K): UiEntry<K> { return structuredClone(this.entries.get(key)!) as UiEntry<K>; }
  subscribe<K extends UiKey>(key: K, listener: (entry: UiEntry<K>) => void) {
    const subscribers = this.listeners.get(key) ?? new Set(); this.listeners.set(key, subscribers);
    subscribers.add(listener as (entry: UiEntry) => void); listener(this.get(key));
    return () => { subscribers.delete(listener as (entry: UiEntry) => void); };
  }
  private update(key: UiKey, patch: Partial<UiEntry>) {
    this.entries.set(key, { ...this.entries.get(key)!, ...patch });
    for (const listener of this.listeners.get(key) ?? []) listener(this.get(key));
  }
  hydrate(): Promise<boolean> {
    if (!this.transport) return Promise.resolve(true);
    if (this.reading) return this.reading;
    this.reading = this.read().finally(() => { this.reading = null; }); return this.reading;
  }
  private async read(): Promise<boolean> {
    try {
      const snapshot = await this.transport!.read();
      if (!object(snapshot) || !exact(snapshot.entries, [...UI_KEYS])) throw Error('The native UI preference snapshot is incomplete.');
      const rows = UI_KEYS.map(key => {
        const row = snapshot.entries[key]; if (!object(row) || !revision(row.revision)) throw Error('The native UI preference revision is invalid.');
        return { key, revision: row.revision, value: validateUiValue(key, row.value === null ? defaults[key] : row.value) };
      });
      for (const row of rows) if (row.revision >= this.entries.get(row.key)!.revision) this.update(row.key, { revision: row.revision, value: row.value, ready: true, error: '' });
      return true;
    } catch (reason) {
      for (const key of UI_KEYS) this.update(key, { ready: false, error: `UI preferences could not be loaded: ${String(reason)}` });
      return false;
    }
  }
  async changed(key: UiKey, nextRevision: number): Promise<void> {
    if (!UI_KEYS.includes(key) || !revision(nextRevision)) return;
    // An event may overtake an already pending read of the previous snapshot.
    for (let attempt = 0; attempt < 2 && nextRevision > this.get(key).revision; attempt++) if (!await this.hydrate()) break;
  }
  async save<K extends UiKey>(key: K, value: UiValues[K]): Promise<UiEntry<K>> {
    const current = this.get(key); value = validateUiValue(key, value);
    if (!current.ready) throw Error('Refresh UI preferences before saving.');
    if (current.busy) throw Error('Wait for the current preference save to finish.');
    this.update(key, { busy: true, error: '' });
    try {
      const saved = this.transport ? await this.transport.write(key, current.revision, value) : { revision: current.revision + 1, value };
      if (!revision(saved.revision) || saved.revision !== current.revision + 1) throw Error('The native preference commit was not acknowledged.');
      const validated = validateUiValue(key, saved.value);
      const same = typeof value === 'object' && value !== null && typeof validated === 'object' && validated !== null
        ? Object.keys(value).length === Object.keys(validated).length && Object.entries(value).every(([field, expected]) => Object.hasOwn(validated, field) && (validated as Record<string, unknown>)[field] === expected)
        : validated === value;
      if (key !== 'remembered' && !same) throw Error('The native preference commit returned a different value.');
      if (saved.revision >= this.get(key).revision) this.update(key, { revision: saved.revision, value: validated, ready: true });
      return this.get(key);
    } catch (reason) {
      this.update(key, { ready: false, error: `Preferences could not be saved: ${String(reason)}. Refresh before retrying.` }); throw reason;
    } finally { this.update(key, { busy: false }); }
  }
}
export const uiPreferences = new UiPreferences();
export function uiPreference<K extends UiKey>(key: K) {
  return { subscribe: (listener: (entry: UiEntry<K>) => void) => uiPreferences.subscribe(key, listener),
    get: () => uiPreferences.get(key), save: (value: UiValues[K]) => uiPreferences.save(key, value) };
}
