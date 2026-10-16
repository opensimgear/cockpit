import test from "node:test";
import assert from "node:assert/strict";
import { COMPANION_LIMITS as L, exampleUi, exampleInputBatch, validatePortableUi, validateInputBatch, validateHello, validateFeedback, parseFrame, publicationHash, comparePublications, neutralValues, validControlValue, isCounter, isPublicationHash, validateBaseline, quantizeAxis } from "../src/index.ts";
const clone = value => structuredClone(value);
const ui = () => clone(exampleUi);
const input = () => clone(exampleInputBatch);
const hash = "sha256:" + "a".repeat(64);
const hello = () => ({ version: 1, type: "hello", clientId: "client-a", sessionId: "session-a", generation: "1", publicationHash: hash, hostId: "cockpit-host", companionId: "companion-a", controllerInstanceId: "instance-a" });
const badUi = (name, mutate) => test("UI rejects " + name, () => { const value = ui(); mutate(value); assert.equal(validatePortableUi(value).ok, false); });

test("portable example is detached, stable and contains no host assignments", async () => {
  const value = ui(), result = validatePortableUi(value);
  assert.equal(result.ok, true);
  value.controls[0].label = "mutated";
  assert.equal(result.value.controls[0].label, "Steering X");
  assert.deepEqual(neutralValues(result.value), { "steering-x": 0, "steering-y": 0, primary: false, secondary: false, "view-hat": -1 });
  const digest = await publicationHash(exampleUi);
  assert.match(digest, /^sha256:[0-9a-f]{64}$/);
  const reordered = Object.fromEntries(Object.entries(ui()).reverse());
  assert.equal(await publicationHash(reordered), digest);
  assert.equal(comparePublications(exampleUi, ui()), "identical");
  const styled = ui(); styled.controls[0].label = "Renamed"; styled.controls[0].style = { accent: "#112233" };
  assert.equal(comparePublications(exampleUi, styled), "styling");
  assert.notEqual(await publicationHash(styled), digest);
  styled.controls[0].fallback = -1;
  assert.equal(comparePublications(exampleUi, styled), "capability");
  await assert.rejects(publicationHash({}), /field/);
  assert.throws(() => comparePublications({}, exampleUi), /field/);
});
for (const [name, mutate] of [
  ["version", v => v.schemaVersion = 2],
  ["credentials", v => v.pairingSecret = "secret"],
  ["local target", v => v.controls[0].targetSlot = 2],
  ["prototype ID", v => v.controls[0].id = "__proto__"],
  ["duplicate ID", v => v.pages[0].id = v.controls[0].id],
  ["invalid label", v => v.title = "\n"],
  ["invalid style", v => v.controls[0].style = { accent: "red" }],
  ["unknown style", v => v.controls[0].style = { css: "url(secret)" }],
  ["control count", v => v.controls = Array.from({ length: L.controls + 1 }, () => v.controls[0])],
  ["unknown control", v => v.controls[0].kind = "script"],
  ["axis mode", v => v.controls[0].axisMode = "raw"],
  ["axis fallback", v => v.controls[0].fallback = NaN],
  ["unipolar fallback", v => { v.controls[0].axisMode = "unipolar"; v.controls[0].fallback = -1; }],
  ["hat fallback", v => v.controls[4].fallback = 0],
  ["feedback kind", v => v.feedbackWidgets[0].kind = "html"],
  ["feedback count", v => v.feedbackWidgets = Array(65).fill(v.feedbackWidgets[0])],
  ["page count", v => v.pages = Array(17).fill(v.pages[0])],
  ["page reference", v => v.pages[0].items[0].refId = "missing"],
  ["page kind", v => v.pages[0].items[0].kind = "script"],
  ["page duplicate", v => v.pages[0].items.push(v.pages[0].items[0])],
  ["zero bounds", v => v.pages[0].items[0].bounds.width = 0],
  ["outside bounds", v => v.pages[0].items[0].bounds.x = 1],
  ["negative bounds", v => v.pages[0].items[0].bounds.y = -1],
  ["non-finite bounds", v => v.pages[0].items[0].bounds.y = Infinity],
  ["unplaced control", v => v.pages[0].items.shift()],
  ["asset digest", v => v.assets = [{ id: "asset", sha256: "bad", sizeBytes: 1, mime: "image/png" }]],
  ["asset size", v => v.assets = [{ id: "asset", sha256: "a".repeat(64), sizeBytes: L.assetBytes + 1, mime: "image/png" }]],
  ["asset URL", v => v.assets = [{ id: "asset", sha256: "a".repeat(64), sizeBytes: 1, mime: "image/svg+xml" }]],
  ["asset total", v => v.assets = Array.from({ length: 5 }, (_, i) => ({ id: "asset-" + i, sha256: "a".repeat(64), sizeBytes: L.assetBytes, mime: "image/png" }))],
  ["byte budget", v => v.title = "x".repeat(L.uiBytes)],
]) badUi(name, mutate);
test("styles, asset manifests and unipolar defaults are valid", () => {
  const value = ui();
  value.controls[0].axisMode = "unipolar"; value.controls[0].fallback = 0.25;
  value.controls[0].style = { foreground: "#ffffff", background: "#000000" };
  value.feedbackWidgets[0].style = { accent: "#abcDEF" };
  value.assets = [{ id: "asset", sha256: "1".repeat(64), sizeBytes: L.assetBytes, mime: "image/webp" }];
  assert.equal(validatePortableUi(value).ok, true);
  assert.equal(neutralValues(value)["steering-x"], 0.25);
  for (const invalid of [undefined, null, [], 4]) assert.equal(validatePortableUi(invalid).ok, false);
  const circular = ui(); circular.self = circular; assert.equal(validatePortableUi(circular).ok, false);
});
test("counter and control value boundaries are precise", () => {
  for (const value of ["0", "1", "9007199254740993", "18446744073709551615"]) assert.equal(isCounter(value), true);
  for (const value of ["", "-1", "01", "1e3", "18446744073709551616", 1, null]) assert.equal(isCounter(value), false);
  assert.equal(isPublicationHash(hash), true); assert.equal(isPublicationHash(hash.toUpperCase()), false);
  const [axis, , button, , hat] = ui().controls;
  for (const value of [-1, 0, 1]) assert.equal(validControlValue(axis, value), true);
  for (const value of [1.1, NaN, false]) assert.equal(validControlValue(axis, value), false);
  assert.equal(validControlValue({ ...axis, axisMode: "unipolar" }, -1), false);
  assert.equal(validControlValue(button, true), true); assert.equal(validControlValue(button, 1), false);
  for (const value of [-1, 0, 7]) assert.equal(validControlValue(hat, value), true);
  for (const value of [0.5, 8, -2]) assert.equal(validControlValue(hat, value), false);
});
test("ordered batch preserves press and release of the same control", () => {
  assert.equal(validateInputBatch(input(), exampleUi).ok, true);
  const value = input(); value.changes = Array.from({ length: 64 }, (_, i) => ({ controlId: "primary", value: i % 2 === 0 }));
  assert.equal(validateInputBatch(value, exampleUi).ok, true);
  value.changes.push({ controlId: "primary", value: false }); assert.equal(validateInputBatch(value, exampleUi).ok, false);
});
test("wire frames reject invalid envelope and payload before session handling", () => {
  for (const mutate of [
    v => v.version = 2, v => v.type = "feedback", v => v.clientId = "",
    v => v.sessionId = 2, v => v.generation = "01", v => v.publicationHash = "bad",
    v => v.sequence = "-1", v => v.changes = [], v => v.changes[0].value = 1,
    v => v.changes[0].controlId = "unknown", v => v.changes[0].secret = true,
  ]) { const value = input(); mutate(value); assert.equal(validateInputBatch(value, exampleUi).ok, false); }
  assert.equal(validateHello(hello()).ok, true);
  assert.equal(validateHello({ ...hello(), token: "no" }).ok, false);
  assert.equal(parseFrame(JSON.stringify(input()), v => validateInputBatch(v, exampleUi)).ok, true);
  assert.equal(parseFrame("{", validateHello).ok, false);
  assert.equal(parseFrame(" ".repeat(L.frameBytes + 1), validateHello).ok, false);
});

const wireIdentity = () => ({ version: 1, clientId: "client-a", sessionId: "session-a", generation: "1", publicationHash: hash });
test("feedback includes provenance, unit/range and source age with widget-typed values", () => {
  const frame = { ...wireIdentity(), type: "feedback", sequence: "1", values: [{ channelId: "engine", value: "Running", provenance: "game-confirmed", sourceKind: "simulator", sourceAgeMs: 0, unit: "status" }] };
  assert.equal(validateFeedback(frame, exampleUi).ok, true);
  for (const mutate of [
    f => f.values[0].value = null, f => f.values[0].value = true,
    f => f.values[0].value = "x".repeat(257), f => f.values[0].provenance = "remote-code",
    f => f.values[0].sourceAgeMs = -1, f => f.values[0].unit = "",
    f => f.values.push({ ...f.values[0] }), f => f.values[0].channelId = "unknown",
  ]) { const value = clone(frame); mutate(value); assert.equal(validateFeedback(value, exampleUi).ok, false); }
  const gaugeUi = ui(); gaugeUi.feedbackWidgets[0].kind = "gauge";
  const gauge = clone(frame); gauge.values[0].value = 50; gauge.values[0].range = { min: 0, max: 100 };
  assert.equal(validateFeedback(gauge, gaugeUi).ok, true);
  gauge.values[0].value = 101; assert.equal(validateFeedback(gauge, gaugeUi).ok, false);
  gauge.values[0].value = 50; gauge.values[0].range = { min: 100, max: 100 }; assert.equal(validateFeedback(gauge, gaugeUi).ok, false);
  const indicatorUi = ui(); indicatorUi.feedbackWidgets[0].kind = "indicator";
  const indicator = clone(frame); indicator.values[0].value = true; assert.equal(validateFeedback(indicator, indicatorUi).ok, true);
});
test("full neutral baseline is a separate bounded frame supporting all 128 controls", () => {
  const value = ui(); value.controls = Array.from({ length: 128 }, (_, i) => ({ id: "button-" + i, label: "Button " + i, kind: "button" }));
  value.pages[0].items = value.controls.map(control => ({ kind: "control", refId: control.id, bounds: { x: 0, y: 0, width: 1, height: 1 } }));
  value.feedbackWidgets = [];
  const frame = { ...wireIdentity(), type: "baseline", sequence: "0", values: neutralValues(value) };
  assert.equal(validatePortableUi(value).ok, true); assert.equal(validateBaseline(frame, value).ok, true);
  frame.sequence = "1"; assert.equal(validateBaseline(frame, value).ok, false);
  frame.sequence = "0"; frame.values["button-1"] = true; assert.equal(validateBaseline(frame, value).ok, false);
});
test("JSON coercion cannot replace the validated object with unvalidated payload", () => {
  const value = ui(); value.toJSON = () => ({ schemaVersion: 999, hostSecret: "reject" });
  assert.equal(validatePortableUi(value).ok, false);
});
test("axis mapping quantization clamps endpoints, uses axis fallback and rounds ties away from zero", () => {
  assert.equal(quantizeAxis(-1, "bipolar", 0), -32768);
  assert.equal(quantizeAxis(1, "bipolar", 0), 32767);
  assert.equal(quantizeAxis(-0.5, "bipolar", 0), -16384);
  assert.equal(quantizeAxis(-0.5 / 32768, "bipolar", 0), -1);
  assert.equal(quantizeAxis(0.5 / 32767, "bipolar", 0), 1);
  assert.equal(quantizeAxis(-10, "unipolar", 0), 0);
  assert.equal(quantizeAxis(10, "bipolar", 0), 32767);
  assert.equal(quantizeAxis(NaN, "unipolar", 0.5), 16384);
  assert.throws(() => quantizeAxis(0, "unipolar", -1), /fallback/);
});

test("committed JSON UI and protocol fixtures are valid and bind the exact publication", async () => {
  const { readFile } = await import("node:fs/promises");
  const { validateHeartbeat } = await import("../src/index.ts");
  const read = async name => JSON.parse(await readFile(new URL("../examples/" + name + ".json", import.meta.url), "utf8"));
  const panel = await read("panel"); assert.deepEqual(panel, exampleUi); assert.equal(validatePortableUi(panel).ok, true);
  const validators = { hello: validateHello, baseline: v => validateBaseline(v, panel), input: v => validateInputBatch(v, panel), heartbeat: validateHeartbeat, feedback: v => validateFeedback(v, panel) };
  for (const [name, validate] of Object.entries(validators)) {
    const frame = await read(name); assert.equal(validate(frame).ok, true, name); assert.equal(frame.publicationHash, await publicationHash(panel));
  }
  assert.equal(validateHeartbeat({ ...(await read("heartbeat")), sequence: "01" }).ok, false);
});


test("feedback source and axis conversion reject unsupported semantics", () => {
  assert.throws(() => quantizeAxis(0, "raw", 0), /mode/);
  assert.equal(Object.is(quantizeAxis(-0.0000001, "bipolar", 0), -0), false);
  const frame = { version: 1, type: "feedback", clientId: "client-a", sessionId: "session-a", generation: "1", publicationHash: hash, sequence: "1", values: [{ channelId: "engine", value: "Running", provenance: "game-confirmed", sourceKind: "untrusted", sourceAgeMs: 0, unit: "status" }] };
  assert.equal(validateFeedback(frame, exampleUi).ok, false);
});

test("stable IDs accept native UUIDs without permitting prototype path keys", () => {
  const frame = hello(); frame.hostId = "7d789ff8-7d35-4b4b-a13f-addd062f54ca";
  frame.companionId = "1fa85f39-aa50-4dc9-a40f-96fd1b386485";
  assert.equal(validateHello(frame).ok, true);
  frame.hostId = "__proto__"; assert.equal(validateHello(frame).ok, false);
});
