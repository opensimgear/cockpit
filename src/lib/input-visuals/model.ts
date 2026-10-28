export type InputStatus = 'value' | 'neutral' | 'waiting' | 'unknown';

/** The geometry is always finite, including while no report is available. */
export function axisVisual(value: number | null | undefined, min: number, max: number) {
  const validRange = Number.isFinite(min) && Number.isFinite(max) && max > min && Number.isFinite(max - min);
  const status: InputStatus = value === undefined ? 'waiting'
    : typeof value === 'number' && Number.isFinite(value) && validRange ? 'value' : 'unknown';
  const lower = validRange ? min : 0;
  const upper = validRange ? max : 1;
  const bipolar = lower < 0 && upper > 0;
  const zero = bipolar ? (0 - lower) / (upper - lower) * 100 : 0;
  const clamped = Math.min(upper, Math.max(lower, status === 'value' ? value as number : 0));
  const position = (clamped - lower) / (upper - lower) * 100;
  return {
    status, bipolar, zero, position, clamped,
    fillStart: Math.min(zero, position), fillSize: Math.abs(position - zero),
    valueText: status === 'value' ? String(value) : status === 'waiting' ? 'Waiting for report' : 'Unknown',
  };
}

export const HAT_DIRECTIONS = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'] as const;
export const HAT_CELLS = [
  { value: 7, text: 'NW', icon: '↖' }, { value: 0, text: 'N', icon: '↑' }, { value: 1, text: 'NE', icon: '↗' },
  { value: 6, text: 'W', icon: '←' }, { value: -1, text: 'Neutral', icon: '•' }, { value: 2, text: 'E', icon: '→' },
  { value: 5, text: 'SW', icon: '↙' }, { value: 4, text: 'S', icon: '↓' }, { value: 3, text: 'SE', icon: '↘' },
] as const;

export function hatVisual(value: number | null | undefined) {
  const neutral = value === null || value === -1;
  const direction = typeof value === 'number' && Number.isInteger(value) && value >= 0 && value < 8 ? value : null;
  const status: InputStatus = value === undefined ? 'waiting' : neutral ? 'neutral' : direction !== null ? 'value' : 'unknown';
  return {
    status, direction, selected: neutral ? -1 : direction,
    valueText: status === 'waiting' ? 'Waiting for report' : neutral ? 'Neutral'
      : direction !== null ? HAT_DIRECTIONS[direction] : 'Unknown',
  };
}
