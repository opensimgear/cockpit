export type ActivityEvent = {
  id: number; occurredAt: number; category: string; eventType: string; level: string;
  message: string; detail: string; entityId: string | null; entityName: string | null; source: string;
};
export type ActivityQuery = {
  search: string; categories: string[]; levels: string[]; eventTypes: string[];
  entityId: string | null; since: number | null; until: number | null; throughId: number | null; offset: number; limit: number;
};
export type ActivityPage = { events: ActivityEvent[]; total: number; throughId: number; categories: string[]; levels: string[]; eventTypes: string[] };
export type ActivitySettings = { retentionDays: number };
export type ActivityRequest = {
  command: 'activity_query' | 'activity_clear' | 'activity_settings_get' | 'activity_settings_save';
  activityFilter?: ActivityQuery; retentionDays?: number;
};
type ActivityReply = { activity?: ActivityPage; activitySettings?: ActivitySettings; activityDeleted?: number };
export type ActivityTransport = (request: ActivityRequest) => Promise<ActivityReply>;
export type ActivityFilters = {
  categories: string[]; levels: string[]; eventTypes: string[]; entityId: string;
  time: 'all' | 'hour' | 'today' | 'day' | 'week' | 'custom'; from: string; until: string;
};
export const ACTIVITY_PAGE_SIZE = 100;
export function emptyActivityFilters(): ActivityFilters {
  return { categories: [], levels: [], eventTypes: [], entityId: '', time: 'all', from: '', until: '' };
}
export function activityFilterCount(filters: ActivityFilters): number {
  return filters.categories.length + filters.levels.length + filters.eventTypes.length
    + Number(!!filters.entityId.trim()) + Number(filters.time !== 'all');
}
export function activityQuery(filters: ActivityFilters, search = '', offset = 0, now = Date.now()): ActivityQuery {
  let since: number | null = null, until: number | null = null;
  if (filters.time === 'today') {
    const start = new Date(now); start.setHours(0, 0, 0, 0); since = start.getTime();
  } else if (filters.time === 'custom') {
    since = filters.from ? new Date(filters.from).getTime() : null;
    until = filters.until ? new Date(filters.until).getTime() : null;
    if (since !== null && !Number.isFinite(since) || until !== null && !Number.isFinite(until)) throw Error('Choose valid dates.');
    if (since !== null && until !== null && since > until) throw Error('The start time must be before the end time.');
  } else {
    const duration = { hour: 3_600_000, day: 86_400_000, week: 604_800_000 }[filters.time as 'hour' | 'day' | 'week'];
    if (duration) since = now - duration;
  }
  return { search: search.trim(), categories: [...filters.categories], levels: [...filters.levels],
    eventTypes: [...filters.eventTypes], entityId: filters.entityId.trim() || null, since, until, throughId: null, offset, limit: ACTIVITY_PAGE_SIZE };
}
export function activityLabel(value: string): string {
  return value.replace(/[._-]+/g, ' ').replace(/\b\w/g, letter => letter.toUpperCase());
}
export class ActivityClient {
  constructor(private transport: ActivityTransport) {}
  async list(query: ActivityQuery): Promise<ActivityPage> {
    const reply = await this.transport({ command: 'activity_query', activityFilter: query });
    if (!reply.activity) throw Error('The activity log is unavailable.');
    return reply.activity;
  }
  async settings(): Promise<ActivitySettings> {
    const reply = await this.transport({ command: 'activity_settings_get' });
    if (!reply.activitySettings) throw Error('Activity retention settings are unavailable.');
    return reply.activitySettings;
  }
  async saveSettings(retentionDays: number): Promise<ActivitySettings> {
    if (!Number.isInteger(retentionDays) || retentionDays < 1 || retentionDays > 3650) throw Error('Retention must be a whole number from 1 to 3650 days.');
    const reply = await this.transport({ command: 'activity_settings_save', retentionDays });
    if (!reply.activitySettings) throw Error('Activity retention settings could not be saved.');
    return reply.activitySettings;
  }
  async clear(): Promise<number> {
    const reply = await this.transport({ command: 'activity_clear' });
    if (typeof reply.activityDeleted !== 'number') throw Error('The activity log could not be cleared.');
    return reply.activityDeleted;
  }
}
export type ActivityLogState = { page: ActivityPage | null; query: ActivityQuery | null; loading: boolean; clearing: boolean; error: string };
export class ActivityLogController {
  private value: ActivityLogState = { page: null, query: null, loading: false, clearing: false, error: '' };
  private epoch = 0;
  private disposed = false;
  private listeners = new Set<(state: ActivityLogState) => void>();
  constructor(private client: ActivityClient) {}
  get state(): ActivityLogState { return this.value; }
  subscribe(listener: (state: ActivityLogState) => void): () => void {
    this.listeners.add(listener); listener(this.value); return () => this.listeners.delete(listener);
  }
  private set(patch: Partial<ActivityLogState>): void {
    if (this.disposed) return;
    this.value = { ...this.value, ...patch }; this.listeners.forEach(listener => listener(this.value));
  }
  async load(query: ActivityQuery): Promise<boolean> {
    if (this.disposed || this.value.clearing) return false;
    const epoch = ++this.epoch;
    this.set({ loading: true, error: '' });
    try {
      const page = await this.client.list(query);
      if (epoch === this.epoch && !this.disposed) { this.set({ page, query: structuredClone(query), loading: false }); return true; }
    } catch (error) {
      if (epoch === this.epoch) this.set({ loading: false, error: error instanceof Error ? error.message : String(error) });
    }
    return false;
  }
  async clear(): Promise<boolean> {
    if (this.disposed || this.value.clearing) return false;
    const epoch = ++this.epoch;
    this.set({ clearing: true, loading: false, error: '' });
    try {
      await this.client.clear();
      if (epoch === this.epoch) this.set({ clearing: false, query: null, page: { events: [], total: 0, throughId: 0, categories: [], levels: [], eventTypes: [] } });
      return true;
    } catch (error) {
      if (epoch === this.epoch) this.set({ clearing: false, error: error instanceof Error ? error.message : String(error) });
      return false;
    }
  }
  dispose(): void { this.disposed = true; this.epoch++; this.listeners.clear(); }
}
