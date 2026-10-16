/** @typedef {'button'|'axis'|'hat'} ControlKind */
/** @typedef {{id: string, kind: ControlKind, value: number|boolean}} Change */
/** @typedef {{sequence: number, atMs: number, reason: string, changes: Change[]}} RecordEntry */

/**
 * Recording-only input owner with no transport, timer or native output dependency.
 * Each pointer owns a group so a second finger cannot release another finger's controls.
 * @param {{clock?: () => number, limit?: number}} options
 */
export function createPreviewRecorder({ clock = () => performance.now(), limit = 24 } = {}) {
  let armed = false;
  let sequence = 0;
  let totalChanges = 0;
  let lastReason = 'Preview has not started';
  /** @type {Map<number, {group: string, controls: Map<string, ControlKind>}>} */
  const pointers = new Map();
  /** @type {Map<string, {kind: ControlKind, value: number|boolean}>} */
  const values = new Map();
  /** @type {RecordEntry[]} */
  const records = [];
  /** @param {ControlKind} kind */
  function fallback(kind) { return kind === 'button' ? false : kind === 'hat' ? -1 : 0; }
  /** @param {Change} change */
  function valid(change) {
    return typeof change.id === 'string' && change.id.length > 0 && (
      change.kind === 'button' ? typeof change.value === 'boolean' :
      change.kind === 'axis' ? typeof change.value === 'number' && Number.isFinite(change.value) && Math.abs(change.value) <= 1 :
      change.kind === 'hat' && typeof change.value === 'number' && Number.isInteger(change.value) && change.value >= -1 && change.value <= 7
    );
  }
  /** @param {Change[]} changes @param {string} reason */
  function record(changes, reason) {
    const changed = changes.filter((change) => values.get(change.id)?.value !== change.value);
    if (!changed.length) return;
    for (const change of changed) values.set(change.id, { kind: change.kind, value: change.value });
    totalChanges += changed.length;
    records.push({ sequence: ++sequence, atMs: Math.round(clock()), reason, changes: changed.map((change) => ({ ...change })) });
    if (records.length > limit) records.splice(0, records.length - limit);
    lastReason = reason;
  }
  function snapshot() {
    return {
      armed, activePointers: pointers.size, totalChanges, lastReason,
      values: Object.fromEntries([...values].map(([id, entry]) => [id, entry.value])),
      records: records.map((entry) => ({ ...entry, changes: entry.changes.map((change) => ({ ...change })) })),
    };
  }
  /** @param {string} reason */
  function disarm(reason = 'Preview stopped') {
    pointers.clear();
    record([...values].map(([id, entry]) => ({ id, kind: entry.kind, value: fallback(entry.kind) })), reason);
    armed = false;
    lastReason = reason;
    return snapshot();
  }
  function arm() {
    disarm('New local preview');
    armed = true;
    lastReason = 'Local preview started';
    return snapshot();
  }
  /** @param {number} pointerId @param {string} group @param {Change[]} changes */
  function press(pointerId, group, changes) {
    if (!armed || !Number.isSafeInteger(pointerId) || pointers.has(pointerId) || !changes.length || !changes.every(valid)) return false;
    if ([...pointers.values()].some((owner) => owner.group === group || changes.some((change) => owner.controls.has(change.id)))) return false;
    if (new Set(changes.map((change) => change.id)).size !== changes.length) return false;
    for (const change of changes) {
      const prior = values.get(change.id);
      if (prior && prior.kind !== change.kind) return false;
    }
    pointers.set(pointerId, { group, controls: new Map(changes.map((change) => [change.id, change.kind])) });
    record(changes, 'Pointer pressed');
    return true;
  }
  /** @param {number} pointerId @param {Change[]} changes */
  function move(pointerId, changes) {
    const owner = pointers.get(pointerId);
    if (!armed || !owner || !changes.length || !changes.every((change) => valid(change) && owner.controls.get(change.id) === change.kind)) return false;
    record(changes, 'Pointer moved');
    return true;
  }
  /** @param {number} pointerId @param {string} reason */
  function release(pointerId, reason = 'Pointer released') {
    const owner = pointers.get(pointerId);
    if (!owner) return false;
    pointers.delete(pointerId);
    record([...owner.controls].map(([id, kind]) => ({ id, kind, value: fallback(kind) })), reason);
    return true;
  }
  return { arm, disarm, press, move, release, snapshot };
}
