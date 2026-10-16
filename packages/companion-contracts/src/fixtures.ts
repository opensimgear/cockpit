import type { InputBatch, PortableUi } from "./contracts.ts";
export const exampleUi: PortableUi = {
  schemaVersion: 1, id: "flight-panel", title: "Companion feasibility panel",
  controls: [
    { id: "steering-x", label: "Steering X", kind: "axis", axisMode: "bipolar", fallback: 0 },
    { id: "steering-y", label: "Steering Y", kind: "axis", axisMode: "bipolar", fallback: 0 },
    { id: "primary", label: "Primary", kind: "button" },
    { id: "secondary", label: "Secondary", kind: "button" },
    { id: "view-hat", label: "View", kind: "hat", fallback: -1 },
  ],
  pages: [{ id: "main-page", title: "Main", items: [
    { kind: "control", refId: "steering-x", bounds: { x: 0.02, y: 0.02, width: 0.45, height: 0.20 } },
    { kind: "control", refId: "steering-y", bounds: { x: 0.02, y: 0.26, width: 0.45, height: 0.20 } },
    { kind: "control", refId: "primary", bounds: { x: 0.53, y: 0.02, width: 0.20, height: 0.20 } },
    { kind: "control", refId: "secondary", bounds: { x: 0.77, y: 0.02, width: 0.20, height: 0.20 } },
    { kind: "control", refId: "view-hat", bounds: { x: 0.53, y: 0.26, width: 0.44, height: 0.44 } },
    { kind: "feedback", refId: "engine-status", bounds: { x: 0.02, y: 0.76, width: 0.95, height: 0.20 } },
  ] }],
  feedbackWidgets: [{ id: "engine-status", kind: "text", label: "Engine", channelId: "engine" }],
  assets: [],
};
/** Placeholder publication digest; compute publicationHash(exampleUi) before publishing. */
export const exampleInputBatch: InputBatch = {
  version: 1, type: "input", clientId: "companion-a", sessionId: "session-a",
  generation: "1", sequence: "1", publicationHash: "sha256:" + "0".repeat(64),
  changes: [{ controlId: "primary", value: true }, { controlId: "primary", value: false }],
};
