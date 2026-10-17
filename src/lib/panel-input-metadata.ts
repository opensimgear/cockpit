/** Bounded native event metadata; an unsupported capability is never an input snapshot. */
export type PanelInputSnapshot = {
  boot: number; stream: number; head: number; buttonCount: number; contactButtonCount?: number;
  held: boolean[]; encoderButtons: number[]; maintainedButtons?: number[]; selectorButtons?: number[][];
  axisCount?: number; axes?: number[]; axisAvailable?: boolean[]; axisNeutral?: number[];
  axisUsage?: number[]; sampledAtMs?: number;
};
export type PanelInputUnsupported = {
  status: 'unsupported'; boot: number; route: [number, number, number, number]; configurationGeneration: number | null;
};
export type PanelInputState = PanelInputSnapshot | PanelInputUnsupported;
export type PanelInputMetadata = Pick<PanelInputSnapshot, 'boot' | 'buttonCount' | 'contactButtonCount' | 'encoderButtons' | 'maintainedButtons' | 'axisCount' | 'axes'>;

/** Selectors consume only bounded advertised metadata, never an unsupported marker. */
export function panelInputMetadata(value: unknown): PanelInputMetadata | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const data = value as Record<string, unknown>;
  if ('status' in data || 'ok' in data && data.ok !== true) return null;
  const count = (value: unknown, max: number): value is number => typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= max;
  if (!count(data.boot, 0xffffffff) || data.boot === 0 || !count(data.buttonCount, 128) || !Array.isArray(data.encoderButtons) || data.encoderButtons.length > 64 || data.encoderButtons.length % 2 !== 0 || !data.encoderButtons.every(index => count(index, (data.buttonCount as number) - 1)) || new Set(data.encoderButtons).size !== data.encoderButtons.length) return null;
  if (data.contactButtonCount !== undefined && !count(data.contactButtonCount, data.buttonCount)) return null;
  if (data.maintainedButtons !== undefined && (!Array.isArray(data.maintainedButtons) || !data.maintainedButtons.every(index => count(index, (data.contactButtonCount as number ?? 0) - 1)))) return null;
  if (data.axisCount !== undefined && !count(data.axisCount, 8)) return null;
  if (data.axes !== undefined && (!Array.isArray(data.axes) || data.axes.length > 8 || !data.axes.every(value => count(value, 65535)) || data.axisCount !== undefined && data.axes.length !== data.axisCount)) return null;
  return value as PanelInputMetadata;
}
