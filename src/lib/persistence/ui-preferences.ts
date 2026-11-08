/** Revisioned preferences shared by the native and browser service transports. */
export const UI_KEYS = ['theme', 'devices-mode', 'canvas-navigation', 'canvas-styling', 'remembered', 'tray', 'progress', 'game-process-events'] as const;
export type UiKey = typeof UI_KEYS[number];
export const OBSERVED_UI_KEYS = ['theme', 'devices-mode', 'canvas-navigation', 'canvas-styling', 'tray', 'progress', 'remembered', 'game-process-events'] as const;
export type UiValues = {
  theme: boolean;
  'devices-mode': 'list' | 'topology';
  'canvas-navigation': { wheelAction: 'pan' | 'zoom' };
  'canvas-styling': { arrowStyle: 'smooth' | 'orthogonal'; cornerRadius: number };
  remembered: { components: unknown[]; forgotten: Record<string, unknown> } | null;
  tray: { closeToTray: boolean; minimizeToTray: boolean };
  progress: { delivery: 'system' | 'window'; position: 'bottom-right' | 'bottom-left' | 'top-right' | 'top-left' };
  'game-process-events': boolean;
};
export type UiEntry<K extends UiKey = UiKey> = { revision: number; value: UiValues[K]; ready: boolean; busy: boolean; error: string };
export type UiSnapshot = { entries: Record<UiKey, { revision: number; value: unknown }> };
export type UiTransport = {
  read(): Promise<UiSnapshot>;
  readRemembered?(): Promise<UiSnapshot['entries']['remembered']>;
  write(key: UiKey, expectedRevision: number, value: unknown): Promise<{ revision: number; value: unknown }>;
};
const defaults: UiValues = {
  theme: false, 'devices-mode': 'list', 'canvas-navigation': { wheelAction: 'pan' },
  'canvas-styling': { arrowStyle: 'smooth', cornerRadius: 24 }, remembered: null,
  tray: { closeToTray: false, minimizeToTray: false }, progress: { delivery: 'system', position: 'bottom-right' },
  'game-process-events': false,
};
const revision = (value: unknown): value is number => Number.isSafeInteger(value) && Number(value) >= 0;
const wireRevision = (value: unknown): value is string =>
  typeof value === 'string' && /^(0|[1-9][0-9]{0,15})$/.test(value) && revision(Number(value));
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
    case 'canvas-styling': valid = exact(value, ['arrowStyle', 'cornerRadius']) && ['smooth', 'orthogonal'].includes(String(value.arrowStyle)) && typeof value.cornerRadius === 'number' && Number.isFinite(value.cornerRadius) && value.cornerRadius >= 0 && value.cornerRadius <= 100; break;
    case 'remembered': valid = value === null || exact(value, ['components', 'forgotten']) && Array.isArray(value.components) && value.components.length <= 1024 && object(value.forgotten) && Object.keys(value.forgotten).length <= 4096; break;
    case 'tray': valid = exact(value, ['closeToTray', 'minimizeToTray']) && typeof value.closeToTray === 'boolean' && typeof value.minimizeToTray === 'boolean'; break;
    case 'progress': valid = exact(value, ['delivery', 'position']) && ['system', 'window'].includes(String(value.delivery)) && ['bottom-right', 'bottom-left', 'top-right', 'top-left'].includes(String(value.position)); break;
    case 'game-process-events': valid = typeof value === 'boolean'; break;
  }
  if (!valid) throw Error(`Saved ${key} preferences are invalid. Preserve the SQLite record and refresh.`);
  return structuredClone(value) as UiValues[K];
}
export class UiPreferences {
  private transport: UiTransport | null;
  private entries = new Map<UiKey, UiEntry>();
  private listeners = new Map<UiKey, Set<(entry: UiEntry) => void>>();
  private reading = new Map<string, Promise<boolean>>();
  private observations = new Map<UiKey, number>();
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
    for (const listener of this.listeners.get(key) ?? []) {
      try { listener(this.get(key)); } catch { /* A consumer cannot block the committed state. */ }
    }
  }
  /** Small values are delivered directly. The inventory stream carries only its revision. */
  applyObservation(key: UiKey, payload: unknown): void {
    if (!OBSERVED_UI_KEYS.includes(key as typeof OBSERVED_UI_KEYS[number])) return;
    try {
      if (exact(payload, ['ready', 'error']) && payload.ready === false && typeof payload.error === 'string' && payload.error) {
        this.observationError(key, payload.error); return;
      }
      if (key === 'remembered') {
        if (!exact(payload, ['ready', 'revision']) || payload.ready !== true || !wireRevision(payload.revision)) throw Error('Invalid observed UI preference revision.');
        void this.changed(key, Number(payload.revision));
        return;
      }
      if (!exact(payload, ['ready', 'revision', 'value']) || payload.ready !== true
          || !wireRevision(payload.revision)) throw Error('Invalid observed UI preference revision.');
      const nextRevision = Number(payload.revision);
      const value = validateUiValue(key, payload.value);
      if (nextRevision < this.get(key).revision) return;
      this.observations.set(key, (this.observations.get(key) ?? 0) + 1);
      this.update(key, { revision: nextRevision, value, ready: true, error: '' });
    } catch (reason) { this.observationError(key, String(reason)); }
  }
  observationError(key: UiKey, message: string): void {
    if (!UI_KEYS.includes(key)) return;
    this.observations.set(key, (this.observations.get(key) ?? 0) + 1);
    this.update(key, { ready: false, error: message });
  }
  hydrate(keys: readonly UiKey[] = UI_KEYS): Promise<boolean> {
    if (!this.transport) return Promise.resolve(true);
    const identity = [...keys].sort().join(',');
    const pending = this.reading.get(identity);
    if (pending) return pending;
    const reading = this.read(keys).finally(() => { this.reading.delete(identity); });
    this.reading.set(identity, reading); return reading;
  }
  private async read(keys: readonly UiKey[]): Promise<boolean> {
    const epochs = new Map(keys.map(key => [key, this.observations.get(key) ?? 0]));
    const unchanged = (key: UiKey) => epochs.get(key) === (this.observations.get(key) ?? 0);
    try {
      const transport = this.transport!;
      const entries = keys.length === 1 && keys[0] === 'remembered' && transport.readRemembered
        ? { remembered: await transport.readRemembered() }
        : await transport.read().then(snapshot => {
          if (!object(snapshot) || !exact(snapshot.entries, [...UI_KEYS])) throw Error('The native UI preference snapshot is incomplete.');
          return snapshot.entries;
        });
      const rows = keys.map(key => {
        const row: unknown = (entries as Partial<UiSnapshot['entries']>)[key];
        if (!object(row) || !revision(row.revision)) throw Error('The native UI preference revision is invalid.');
        return { key, revision: row.revision, value: validateUiValue(key, row.value === null ? defaults[key] : row.value) };
      });
      for (const row of rows) if (unchanged(row.key) && row.revision >= this.entries.get(row.key)!.revision) this.update(row.key, { revision: row.revision, value: row.value, ready: true, error: '' });
      return true;
    } catch (reason) {
      for (const key of keys) if (unchanged(key)) this.update(key, { ready: false, error: `UI preferences could not be loaded: ${String(reason)}` });
      return false;
    }
  }
  async changed(key: UiKey, nextRevision: number): Promise<void> {
    if (!UI_KEYS.includes(key) || !revision(nextRevision)) return;
    // An event may overtake an already pending read of the previous snapshot.
    const epoch = this.observations.get(key);
    for (let attempt = 0; attempt < 2 && epoch === this.observations.get(key); attempt++) {
      const entry = this.entries.get(key)!;
      if ((entry.ready && nextRevision <= entry.revision) || !await this.hydrate([key])) break;
    }
  }
  async save<K extends UiKey>(key: K, value: UiValues[K]): Promise<UiEntry<K>> {
    const current = this.get(key); value = validateUiValue(key, value);
    if (!current.ready) throw Error('Refresh UI preferences before saving.');
    if (current.busy) throw Error('Wait for the current preference save to finish.');
    const epoch = this.observations.get(key) ?? 0;
    this.update(key, { busy: true, error: '' });
    try {
      const saved = this.transport ? await this.transport.write(key, current.revision, value) : { revision: current.revision + 1, value };
      if (!revision(saved.revision) || saved.revision !== current.revision + 1) throw Error('The native preference commit was not acknowledged.');
      const validated = validateUiValue(key, saved.value);
      const same = typeof value === 'object' && value !== null && typeof validated === 'object' && validated !== null
        ? Object.keys(value).length === Object.keys(validated).length && Object.entries(value).every(([field, expected]) => Object.hasOwn(validated, field) && (validated as Record<string, unknown>)[field] === expected)
        : validated === value;
      if (key !== 'remembered' && !same) throw Error('The native preference commit returned a different value.');
      if (epoch === (this.observations.get(key) ?? 0) && saved.revision >= this.get(key).revision) this.update(key, { revision: saved.revision, value: validated, ready: true });
      return this.get(key);
    } catch (reason) {
      if (epoch === (this.observations.get(key) ?? 0)) {
        this.update(key, { ready: false, error: `Preferences could not be saved: ${String(reason)}. Refresh before retrying.` });
      }
      throw reason;
    } finally { this.update(key, { busy: false }); }
  }
}
export const uiPreferences = new UiPreferences();
export function uiPreference<K extends UiKey>(key: K) {
  return { subscribe: (listener: (entry: UiEntry<K>) => void) => uiPreferences.subscribe(key, listener),
    get: () => uiPreferences.get(key), save: (value: UiValues[K]) => uiPreferences.save(key, value) };
}
