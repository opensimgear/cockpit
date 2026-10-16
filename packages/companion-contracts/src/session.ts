import { COMPANION_LIMITS, isCounter, isPublicationHash, neutralValues, validateBaseline, validateHello, validateInputBatch, validatePortableUi } from "./contracts.ts";
import type { Hello, InputBatch, PortableUi, Values } from "./contracts.ts";

export type CancelReason = "blur" | "page-hidden" | "touch-cancel" | "pointer-capture-lost" | "disconnect" | "layout-change" | "capability-change";
export interface SessionClient {
  clientId: string; sessionId: string; roleId: string | null; armed: boolean;
  generation: string; generationExhausted: boolean; sequence: string; heartbeatSequence: string; lastSeenMs: number;
  batchTimes: number[]; pendingTransitions: number; values: Values;
  hostId: string; companionId: string; controllerInstanceId: string;
}
export interface SessionState {
  ui: PortableUi; publicationHash: string; generation: string; hostId: string; roles: string[];
  existingRoleCount: number; clients: SessionClient[]; usedSessionIds: string[]; nowMs: number;
}
export type SessionAction =
  | { type: "connect"; hello: Hello; nowMs: number }
  | { type: "claim"; clientId: string; roleId: string; takeover: boolean; nowMs: number }
  | { type: "baseline"; clientId: string; sessionId: string; generation: string; publicationHash: string; sequence: "0"; values: Values; nowMs: number }
  | { type: "input"; batch: InputBatch; nowMs: number }
  | { type: "heartbeat"; clientId: string; sessionId: string; generation: string; sequence: string; nowMs: number }
  | { type: "ack"; clientId: string; sessionId: string; generation: string; count: number; nowMs: number }
  | { type: "cancel"; clientId: string; sessionId: string; generation: string; reason: CancelReason; nowMs: number }
  | { type: "tick"; nowMs: number }
  | { type: "generation"; generation: string; nowMs: number };
export interface SessionEffect {
  kind: "fallback" | "values"; roleId: string; clientId: string; values: Values;
  /** Ordered changes must be delivered in order; values is only the resulting snapshot. */
  changes?: InputBatch["changes"]; reason?: string;
}
export interface SessionResult { state: SessionState; effects: SessionEffect[]; error?: string }

/** Pure model only: a native host must authenticate identity and verify publication bytes first. */
export function createSessionState(ui: PortableUi, publicationHash: string, roles: string[] = ["panel"], existingRoleCount = 0, generation = "1", hostId = "cockpit-host"): SessionState {
  const result = validatePortableUi(ui);
  if (!result.ok) throw new Error(result.error);
  if (!isPublicationHash(publicationHash) || !isCounter(generation) || !/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(hostId)) throw new Error("Invalid session identity");
  if (!Number.isInteger(existingRoleCount) || existingRoleCount < 0 || roles.length + existingRoleCount > COMPANION_LIMITS.roles || roles.length === 0 ||
    new Set(roles).size !== roles.length || roles.some(role => !/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(role))) throw new Error("Invalid role budget");
  return { ui: result.value, publicationHash, generation, hostId, roles: [...roles], existingRoleCount, clients: [], usedSessionIds: [], nowMs: 0 };
}

export function reduceSession(previous: SessionState, action: SessionAction): SessionResult {
  if (!Number.isSafeInteger(action.nowMs) || action.nowMs < previous.nowMs) return { state: previous, effects: [], error: "Non-monotonic host clock" };
  const state: SessionState = { ...previous, nowMs: action.nowMs, usedSessionIds: [...previous.usedSessionIds], clients: previous.clients.map(client => ({ ...client, batchTimes: [...client.batchTimes], values: { ...client.values } })) };
  const effects: SessionEffect[] = [];
  const fail = (error: string): SessionResult => ({ state, effects, error });
  // Every owned-role cleanup invalidates queued baselines, including reservations not yet armed.
  const fallback = (client: SessionClient, reason: string, release = false, invalidatePendingBaseline = client.roleId !== null) => {
    const wasArmed = client.armed;
    if (wasArmed || invalidatePendingBaseline) {
      if (client.generation === "18446744073709551615") client.generationExhausted = true;
      else client.generation = String(BigInt(client.generation) + 1n);
    }
    client.armed = false; client.values = neutralValues(state.ui); client.sequence = "0"; client.pendingTransitions = 0;
    if (client.roleId && wasArmed) effects.push({ kind: "fallback", roleId: client.roleId, clientId: client.clientId, values: { ...client.values }, reason });
    if (release) client.roleId = null;
  };
  // Lease expiry is processed before every event, not only an optional timer call.
  for (const client of state.clients) if (action.nowMs - client.lastSeenMs >= COMPANION_LIMITS.leaseMs) fallback(client, "lease-expired", true, client.roleId !== null);
  const matches = (client: SessionClient | undefined, sessionId: string, generation: string) =>
    client && client.sessionId === sessionId && generation === client.generation;
  if (action.type === "tick") return { state, effects };
  if (action.type === "generation") {
    if (!isCounter(action.generation) || BigInt(action.generation) <= BigInt(state.generation)) return fail("Stale generation");
    for (const client of state.clients) fallback(client, "generation-changed", true);
    state.clients = []; state.generation = action.generation; return { state, effects };
  }
  if (action.type === "connect") {
    const hello = validateHello(action.hello);
    if (!hello.ok) return fail(hello.error);
    if (action.hello.generation !== state.generation || action.hello.publicationHash !== state.publicationHash || action.hello.hostId !== state.hostId) return fail("Stale generation or publication");
    const existing = state.clients.find(client => client.clientId === action.hello.clientId);
    if (state.usedSessionIds.includes(action.hello.sessionId)) return fail("Session ID already used");
    if (state.usedSessionIds.length >= COMPANION_LIMITS.sessionIdsPerRun) return fail("Session identity budget exhausted; start a fresh mapping run");
    if (!existing && state.clients.length >= COMPANION_LIMITS.clients) return fail("Client budget exceeded");
    if (existing) { fallback(existing, "reconnect", true); state.clients = state.clients.filter(client => client !== existing); }
    state.usedSessionIds.push(action.hello.sessionId);
    state.clients.push({
      clientId: action.hello.clientId, sessionId: action.hello.sessionId, roleId: null, armed: false,
      generation: state.generation, generationExhausted: false, sequence: "0", heartbeatSequence: "0", lastSeenMs: action.nowMs,
      batchTimes: [], pendingTransitions: 0, values: neutralValues(state.ui),
      hostId: action.hello.hostId, companionId: action.hello.companionId, controllerInstanceId: action.hello.controllerInstanceId,
    });
    return { state, effects };
  }
  const clientId = action.type === "input" ? action.batch.clientId : action.clientId;
  const client = state.clients.find(client => client.clientId === clientId);
  if (!client) return fail("Unknown client");
  if (action.type === "claim") {
    if (!state.roles.includes(action.roleId)) return fail("Unknown role");
    const owners = state.clients.filter(other => other !== client && other.roleId !== null && (other.roleId === action.roleId || other.controllerInstanceId === client.controllerInstanceId));
    if (owners.length > 0 && !action.takeover) return fail("Role already owned");
    if (owners.length === 0 && !client.roleId && state.clients.filter(other => other.roleId !== null).length >= COMPANION_LIMITS.targets) return fail("Target budget exceeded");
    for (const owner of owners) fallback(owner, "explicit-takeover", true);
    fallback(client, "role-changed", true); client.roleId = action.roleId; client.lastSeenMs = action.nowMs;
    return { state, effects };
  }
  if (action.type === "cancel") {
    if (!matches(client, action.sessionId, action.generation)) return fail("Stale cancellation identity");
    fallback(client, action.reason, action.reason === "disconnect", true);
    if (action.reason === "disconnect") state.clients = state.clients.filter(other => other !== client);
    return { state, effects };
  }
  if (action.type === "heartbeat") {
    if (!matches(client, action.sessionId, action.generation) || !isCounter(action.sequence) || BigInt(action.sequence) <= BigInt(client.heartbeatSequence)) return fail("Stale heartbeat");
    if (action.sequence === "18446744073709551615") { fallback(client, "counter-exhausted"); client.generationExhausted = true; return fail("Heartbeat counter exhausted; reconnect with a fresh session"); }
    client.heartbeatSequence = action.sequence; client.lastSeenMs = action.nowMs; return { state, effects };
  }
  if (action.type === "ack") {
    if (!matches(client, action.sessionId, action.generation)) return fail("Stale acknowledgement identity");
    if (!Number.isSafeInteger(action.count) || action.count < 0 || action.count > client.pendingTransitions) return fail("Invalid transition acknowledgement");
    client.pendingTransitions -= action.count; return { state, effects };
  }
  if (action.type === "baseline") {
    if (!matches(client, action.sessionId, action.generation) || action.publicationHash !== state.publicationHash || action.sequence !== "0") return fail("Stale baseline identity");
    if (client.generationExhausted) return fail("Generation exhausted; reconnect with a fresh session");
    if (!client.roleId || client.armed) return fail("Baseline requires an owned disarmed role");
    const baseline = validateBaseline({ version: 1, type: "baseline", clientId, sessionId: action.sessionId, generation: action.generation, publicationHash: action.publicationHash, sequence: action.sequence, values: action.values }, state.ui);
    if (!baseline.ok) return fail("Baseline must include every control at its fallback: " + baseline.error);
    const neutral = neutralValues(state.ui);
    if (!action.values || Object.keys(action.values).length !== Object.keys(neutral).length || Object.entries(neutral).some(([id, value]) => action.values[id] !== value)) return fail("Baseline must include every control at its fallback");
    client.values = { ...neutral }; client.sequence = "0"; client.armed = true; client.lastSeenMs = action.nowMs;
    client.batchTimes = client.batchTimes.filter(time => action.nowMs - time < 1000); client.pendingTransitions = 0;
    effects.push({ kind: "values", roleId: client.roleId, clientId, values: { ...neutral }, changes: [] });
    return { state, effects };
  }
  // Old malformed frames cannot cancel a replacement writer.
  if (!matches(client, action.batch.sessionId, action.batch.generation) || action.batch.publicationHash !== state.publicationHash) return fail("Stale input identity");
  const result = validateInputBatch(action.batch, state.ui);
  if (!result.ok) { fallback(client, "invalid-input"); return fail(result.error); }
  const batch = result.value;
  if (!matches(client, batch.sessionId, batch.generation) || batch.publicationHash !== state.publicationHash) return fail("Stale input identity");
  if (!client.roleId || !client.armed) return fail("Client must claim and rearm");
  const sequence = BigInt(batch.sequence), last = BigInt(client.sequence);
  if (sequence === 18446744073709551615n) { fallback(client, "counter-exhausted"); return fail("Input counter exhausted; rearm at the new generation"); }
  if (sequence <= last) return fail("Stale input sequence");
  if (sequence !== last + 1n) { fallback(client, "sequence-gap"); return fail("Input sequence gap"); }
  client.batchTimes = client.batchTimes.filter(time => action.nowMs - time < 1000);
  if (client.batchTimes.length >= COMPANION_LIMITS.inputBatchesPerSecond) { fallback(client, "rate-limit"); return fail("Input rate exceeded"); }
  if (client.pendingTransitions + batch.changes.length > COMPANION_LIMITS.queuedTransitions) { fallback(client, "queue-overflow"); return fail("Ordered transition queue overflow"); }
  for (const change of batch.changes) client.values[change.controlId] = change.value;
  client.sequence = batch.sequence; client.batchTimes.push(action.nowMs); client.pendingTransitions += batch.changes.length; client.lastSeenMs = action.nowMs;
  effects.push({ kind: "values", roleId: client.roleId, clientId, values: { ...client.values }, changes: batch.changes });
  return { state, effects };
}
