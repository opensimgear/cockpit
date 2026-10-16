import test from "node:test";
import assert from "node:assert/strict";
import { createSessionState, reduceSession, neutralValues, exampleUi, exampleInputBatch, COMPANION_LIMITS as L } from "../src/index.ts";
const hash = "sha256:" + "a".repeat(64);
const hello = (clientId = "client-a", sessionId = "session-a") => ({ version: 1, type: "hello", clientId, sessionId, generation: "1", publicationHash: hash, hostId: "cockpit-host", companionId: clientId, controllerInstanceId: clientId + "-instance" });
function harness(roles = ["panel-a", "panel-b"], ui = exampleUi) {
  let state = createSessionState(ui, hash, roles); let nowMs = 0;
  const send = action => { const client = state.clients.find(client => client.clientId === action.clientId); const identity = action.type === "cancel" || action.type === "ack" ? { sessionId: client?.sessionId, generation: client?.generation } : {}; const result = reduceSession(state, { nowMs, ...identity, ...action }); state = result.state; return result; };
  const connect = (clientId = "client-a", sessionId = "session-a") => send({ type: "connect", hello: hello(clientId, sessionId) });
  const claim = (clientId = "client-a", roleId = "panel-a", takeover = false) => send({ type: "claim", clientId, roleId, takeover });
  const baseline = (clientId = "client-a", sessionId = "session-a", values = neutralValues(ui)) => send({ type: "baseline", clientId, sessionId, generation: state.clients.find(client => client.clientId === clientId)?.generation ?? "1", sequence: "0", publicationHash: hash, values });
  const input = (sequence = "1", changes = [{ controlId: "primary", value: true }], extra = {}) => send({ type: "input", batch: { ...exampleInputBatch, generation: state.clients.find(client => client.clientId === "client-a")?.generation ?? "1", clientId: "client-a", sessionId: "session-a", publicationHash: hash, sequence, changes, ...extra } });
  const ready = () => { connect(); claim(); baseline(); };
  return { send, connect, claim, baseline, input, ready, get state() { return state; }, advance(ms) { nowMs += ms; } };
}
test("constructor rejects invalid metadata and preserves detached UI", () => {
  for (const args of [[{}, hash], [exampleUi, "bad"], [exampleUi, hash, []], [exampleUi, hash, ["a", "a"]], [exampleUi, hash, ["a"], 16], [exampleUi, hash, ["a"], -1], [exampleUi, hash, ["a"], 0, "01"], [exampleUi, hash, ["__proto__"]]]) assert.throws(() => createSessionState(...args));
  const ui = structuredClone(exampleUi), state = createSessionState(ui, hash); ui.controls[0].label = "Changed";
  assert.equal(state.ui.controls[0].label, "Steering X");
});
test("requires ownership and complete neutral baseline before ordered input", () => {
  const h = harness(); h.connect();
  assert.match(h.baseline().error, /owned/);
  assert.match(h.claim("client-a", "missing").error, /Unknown role/);
  h.claim(); assert.match(h.input().error, /rearm/);
  assert.match(h.baseline("client-a", "session-a", { primary: false }).error, /every control/);
  assert.match(h.baseline("client-a", "session-a", { ...neutralValues(exampleUi), primary: true }).error, /every control/);
  assert.equal(h.baseline().error, undefined);
  assert.match(h.baseline().error, /disarmed/);
  const previous = structuredClone(h.state);
  const result = h.input("1", [{ controlId: "primary", value: true }, { controlId: "primary", value: false }]);
  assert.equal(result.effects[0].changes.length, 2); assert.equal(result.effects[0].values.primary, false);
  assert.deepEqual(previous.clients[0].values, neutralValues(exampleUi));
  assert.equal(h.state.clients[0].pendingTransitions, 2);
  h.send({ type: "ack", clientId: "client-a", count: 2 }); assert.equal(h.state.clients[0].pendingTransitions, 0);
  assert.match(h.send({ type: "ack", clientId: "client-a", count: 1 }).error, /acknowledgement/);
});
test("sequence gaps fail closed with axis-specific fallback and explicit rearm", () => {
  const ui = structuredClone(exampleUi); ui.controls[1].axisMode = "unipolar"; ui.controls[1].fallback = 0.15;
  const h = harness(undefined, ui); h.ready();
  h.input("1", [{ controlId: "steering-y", value: 1 }, { controlId: "primary", value: true }]);
  const gap = h.input("3");
  assert.match(gap.error, /gap/); assert.equal(gap.effects[0].values["steering-y"], 0.15); assert.equal(gap.effects[0].values.primary, false);
  assert.equal(h.state.clients[0].armed, false);
  assert.match(h.input("4").error, /rearm/);
  h.baseline(); assert.equal(h.input().error, undefined);
  assert.match(h.input().error, /Stale input sequence/);
  assert.equal(h.state.clients[0].armed, true);
});
test("independent clients own separate roles and takeover requires explicit consent", () => {
  const h = harness(); h.ready(); h.input();
  h.connect("client-b", "session-b");
  assert.match(h.claim("client-b").error, /already owned/);
  h.claim("client-b", "panel-b"); h.baseline("client-b", "session-b");
  assert.equal(h.state.clients[0].values.primary, true); assert.equal(h.state.clients[1].values.primary, false);
  const takeover = h.claim("client-b", "panel-a", true);
  assert.equal(takeover.effects.filter(e => e.kind === "fallback").length, 2);
  assert.equal(h.state.clients[0].roleId, null); assert.equal(h.state.clients[1].roleId, "panel-a");
  assert.equal(h.state.clients[1].armed, false);
  h.baseline("client-b", "session-b"); assert.equal(h.state.clients[1].armed, true);
});
test("heartbeat counter is independent and stale generations do not affect active controls", () => {
  const h = harness(); h.ready(); h.input();
  const old = h.input("2", [{ controlId: "primary", value: false }], { generation: "0" });
  assert.match(old.error, /Stale input identity/); assert.equal(h.state.clients[0].values.primary, true);
  h.advance(1000);
  const heartbeat = h.send({ type: "heartbeat", clientId: "client-a", sessionId: "session-a", generation: "1", sequence: "9876543210123456789" });
  assert.equal(heartbeat.error, undefined); assert.equal(h.state.clients[0].sequence, "1");
  assert.match(h.send({ type: "heartbeat", clientId: "client-a", sessionId: "session-a", generation: "1", sequence: "1" }).error, /Stale/);
  h.advance(1000); assert.equal(h.send({ type: "tick" }).effects.length, 0);
  h.advance(500); assert.equal(h.send({ type: "tick" }).effects[0].reason, "lease-expired");
  assert.equal(h.state.clients[0].roleId, null);
});
test("lifecycle cancellation cannot silently rearm and disconnect removes the client", () => {
  for (const reason of ["blur", "page-hidden", "touch-cancel", "pointer-capture-lost", "layout-change", "capability-change", "disconnect"]) {
    const h = harness(); h.ready(); h.input();
    const cancel = h.send({ type: "cancel", clientId: "client-a", reason });
    assert.equal(cancel.effects[0].reason, reason);
    if (reason === "disconnect") assert.equal(h.state.clients.length, 0);
    else { assert.equal(h.state.clients[0].armed, false); assert.match(h.input("2").error, /rearm/); }
  }
});
test("reconnect and host generation retire previous session state", () => {
  const h = harness(); h.ready(); h.input();
  assert.match(h.connect().error, /already used/);
  const reconnect = h.connect("client-a", "session-new");
  assert.equal(reconnect.effects[0].reason, "reconnect");
  assert.equal(h.state.clients.length, 1); assert.equal(h.state.clients[0].roleId, null);
  h.claim(); assert.match(h.baseline().error, /Stale baseline/); h.baseline("client-a", "session-new");
  assert.match(h.send({ type: "generation", generation: "1" }).error, /Stale/);
  const restart = h.send({ type: "generation", generation: "2" }); assert.equal(restart.effects[0].reason, "generation-changed");
  assert.equal(h.state.clients.length, 0); assert.match(h.connect().error, /Stale generation/);
  assert.match(h.send({ type: "claim", clientId: "missing", roleId: "panel-a", takeover: false }).error, /Unknown client/);
});
test("invalid input, rate limit and ordered queue overflow disarm atomically", () => {
  const invalid = harness(); invalid.ready(); invalid.input(); const result = invalid.input("2", [{ controlId: "primary", value: 1 }]);
  assert.equal(result.effects[0].reason, "invalid-input"); assert.equal(invalid.state.clients[0].values.primary, false);
  const rate = harness(); rate.ready();
  for (let i = 1; i <= L.inputBatchesPerSecond; i++) assert.equal(rate.input(String(i)).error, undefined);
  assert.match(rate.input("121").error, /rate/); assert.equal(rate.state.clients[0].armed, false);
  rate.advance(1000); rate.baseline(); assert.equal(rate.input().error, undefined);
  const queue = harness(); queue.ready();
  const changes = Array.from({ length: 64 }, (_, i) => ({ controlId: "primary", value: i % 2 === 0 }));
  for (let i = 1; i <= 8; i++) assert.equal(queue.input(String(i), changes).error, undefined);
  const overflow = queue.input("9"); assert.equal(overflow.effects[0].reason, "queue-overflow");
});
test("eight independent clients are bounded and non-monotonic clocks rejected", () => {
  const h = harness(Array.from({ length: 8 }, (_, i) => "role-" + i));
  for (let i = 0; i < 8; i++) { assert.equal(h.connect("client-" + i, "session-" + i).error, undefined); assert.equal(h.claim("client-" + i, "role-" + i).error, undefined); }
  assert.match(h.connect("client-extra", "session-extra").error, /budget/);
  h.advance(5); h.send({ type: "tick" }); const state = h.state;
  const backward = h.send({ type: "tick", nowMs: 4 }); assert.equal(backward.state, state); assert.match(backward.error, /clock/);
  assert.match(h.send({ type: "connect", hello: { ...hello(), version: 2 } }).error, /version/);
});


test("disarm rotates only the affected writer and rejects stale input after rebaseline", () => {
  for (const reason of ["blur", "page-hidden", "touch-cancel", "pointer-capture-lost", "layout-change", "capability-change"]) {
    const h = harness(); h.ready(); h.input();
    h.connect("client-b", "session-b"); h.claim("client-b", "panel-b"); h.baseline("client-b", "session-b");
    h.send({ type: "cancel", clientId: "client-a", reason });
    assert.equal(h.state.clients[0].generation, "2"); assert.equal(h.state.clients[1].generation, "1"); assert.equal(h.state.clients[1].armed, true);
    h.baseline();
    const stale = h.input("1", [{ controlId: "primary", value: true }], { generation: "1" });
    assert.match(stale.error, /Stale input identity/); assert.equal(stale.effects.length, 0); assert.equal(h.state.clients[0].values.primary, false);
    assert.equal(h.input("1").error, undefined);
  }
  const h = harness(); h.ready(); h.input("3"); assert.equal(h.state.clients[0].generation, "2");
  h.baseline(); assert.match(h.input("1", undefined, { generation: "1" }).error, /Stale/);
});
test("old sessions cannot be reused or cancel a replacement writer with malformed input", () => {
  const h = harness(); h.ready(); h.input();
  h.connect("client-a", "session-new"); h.claim(); h.baseline("client-a", "session-new");
  assert.match(h.connect().error, /already used/);
  const stale = h.input("2", [{ controlId: "primary", value: "malformed" }]);
  assert.match(stale.error, /Stale input identity/); assert.equal(stale.effects.length, 0); assert.equal(h.state.clients[0].armed, true);
});
test("same controller instance requires takeover even across different roles", () => {
  const h = harness(); h.ready();
  h.send({ type: "connect", hello: { ...hello("client-b", "session-b"), controllerInstanceId: "client-a-instance" } });
  assert.match(h.claim("client-b", "panel-b").error, /owned/);
  const takeover = h.claim("client-b", "panel-b", true);
  assert.equal(takeover.effects[0].reason, "explicit-takeover"); assert.equal(h.state.clients[0].generation, "2");
  assert.equal(h.state.clients[0].roleId, null); assert.equal(h.state.clients[1].roleId, "panel-b");
});
test("identity and sequence exhaustion fail closed without wrapping or unbounded memory", () => {
  const h = harness(); h.ready();
  const rollover = h.input("18446744073709551615"); assert.equal(rollover.effects[0].reason, "counter-exhausted"); assert.equal(h.state.clients[0].generation, "2");
  const maxGeneration = { ...h.state, clients: [{ ...h.state.clients[0], generation: "18446744073709551615", armed: true }] };
  const cancelled = reduceSession(maxGeneration, { type: "cancel", clientId: "client-a", sessionId: "session-a", generation: "18446744073709551615", reason: "blur", nowMs: 0 });
  assert.equal(cancelled.state.clients[0].generationExhausted, true);
  const baseline = reduceSession(cancelled.state, { type: "baseline", clientId: "client-a", sessionId: "session-a", generation: "18446744073709551615", publicationHash: hash, sequence: "0", values: neutralValues(exampleUi), nowMs: 0 });
  assert.match(baseline.error, /Generation exhausted/);
  const full = { ...h.state, usedSessionIds: Array.from({ length: L.sessionIdsPerRun }, (_, i) => "old-" + i) };
  const connect = reduceSession(full, { type: "connect", hello: hello("client-b", "session-b"), nowMs: 0 });
  assert.match(connect.error, /identity budget/);
});
test("rolling input budget cannot double burst across a fixed-window boundary", () => {
  const h = harness(); h.ready(); h.advance(999);
  for (let i = 1; i <= 120; i++) h.input(String(i));
  h.advance(1); assert.match(h.input("121").error, /rate/);
});


test("cancellation and malformed traffic from retired identities cannot disarm current state", () => {
  const h = harness(); h.ready(); h.input(); h.send({ type: "cancel", clientId: "client-a", reason: "blur" }); h.baseline();
  const old = h.send({ type: "cancel", clientId: "client-a", sessionId: "session-a", generation: "1", reason: "blur" });
  assert.match(old.error, /Stale cancellation/); assert.equal(h.state.clients[0].armed, true);
  const wrongHost = h.send({ type: "connect", hello: { ...hello("client-b", "session-b"), hostId: "different-host" } });
  assert.match(wrongHost.error, /Stale/);
});
test("heartbeat counter rollover disarms without affecting input sequence or other clients", () => {
  const h = harness(); h.ready(); h.input();
  const result = h.send({ type: "heartbeat", clientId: "client-a", sessionId: "session-a", generation: "1", sequence: "18446744073709551615" });
  assert.equal(result.effects[0].reason, "counter-exhausted"); assert.equal(h.state.clients[0].armed, false); assert.match(h.baseline().error, /Generation exhausted/);
});



test("rearming cannot reset the rolling client input rate budget", () => {
  const h = harness(); h.ready();
  for (let i = 1; i <= 60; i++) assert.equal(h.input(String(i)).error, undefined);
  h.send({ type: "cancel", clientId: "client-a", reason: "blur" }); h.baseline();
  for (let i = 1; i <= 60; i++) assert.equal(h.input(String(i)).error, undefined);
  assert.match(h.input("61").error, /rate/);
  h.advance(1000); h.baseline(); assert.equal(h.input("1").error, undefined);
});
test("delayed native acknowledgements cannot consume a rearmed generation's queue", () => {
  const h = harness(); h.ready(); h.input();
  const oldAck = { type: "ack", clientId: "client-a", sessionId: "session-a", generation: "1", count: 1 };
  h.send({ type: "cancel", clientId: "client-a", reason: "blur" }); h.baseline(); h.input();
  assert.equal(h.state.clients[0].pendingTransitions, 1);
  assert.match(h.send(oldAck).error, /Stale acknowledgement/);
  assert.equal(h.state.clients[0].pendingTransitions, 1);
  assert.equal(h.send({ type: "ack", clientId: "client-a", count: 1 }).error, undefined);
  assert.equal(h.state.clients[0].pendingTransitions, 0);
});
test("cancel before the first baseline invalidates queued baseline and input", () => {
  const h = harness(); h.connect(); h.claim();
  h.send({ type: "cancel", clientId: "client-a", reason: "page-hidden" });
  assert.equal(h.state.clients[0].generation, "2");
  const queued = h.send({ type: "baseline", clientId: "client-a", sessionId: "session-a", generation: "1", publicationHash: hash, sequence: "0", values: neutralValues(exampleUi) });
  assert.match(queued.error, /Stale baseline/);
  assert.match(h.input("1", undefined, { generation: "1" }).error, /Stale input/);
  assert.equal(h.baseline().error, undefined);
});
test("lease expiry invalidates an awaiting baseline once", () => {
  const h = harness(); h.connect(); h.claim(); h.advance(1500); h.send({ type: "tick" });
  assert.equal(h.state.clients[0].generation, "2"); assert.equal(h.state.clients[0].roleId, null);
  h.advance(1); h.send({ type: "tick" }); assert.equal(h.state.clients[0].generation, "2");
});

test("role changes and same-role reclaim invalidate a reserved but unarmed baseline", () => {
  for (const roleId of ["panel-a", "panel-b"]) {
    const h = harness(); h.connect(); h.claim();
    h.claim("client-a", roleId);
    assert.equal(h.state.clients[0].generation, "2");
    const oldBaseline = h.send({ type: "baseline", clientId: "client-a", sessionId: "session-a", generation: "1", publicationHash: hash, sequence: "0", values: neutralValues(exampleUi) });
    assert.match(oldBaseline.error, /Stale baseline/);
    assert.match(h.input("1", undefined, { generation: "1" }).error, /Stale input/);
    assert.equal(h.baseline().error, undefined);
    assert.equal(h.state.clients[0].roleId, roleId);
  }
});
test("takeover invalidates the old owner's queued unarmed baseline before later reclaim", () => {
  const h = harness(); h.connect(); h.claim();
  h.connect("client-b", "session-b"); h.claim("client-b", "panel-a", true);
  assert.equal(h.state.clients[0].generation, "2"); assert.equal(h.state.clients[0].roleId, null);
  h.claim("client-a", "panel-b");
  const oldBaseline = h.send({ type: "baseline", clientId: "client-a", sessionId: "session-a", generation: "1", publicationHash: hash, sequence: "0", values: neutralValues(exampleUi) });
  assert.match(oldBaseline.error, /Stale baseline/);
  assert.equal(h.baseline().error, undefined);
  assert.equal(h.state.clients[1].generation, "1"); assert.equal(h.state.clients[1].roleId, "panel-a");
});
