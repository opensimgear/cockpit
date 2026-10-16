import { writeFile } from "node:fs/promises";
import { exampleUi, exampleInputBatch, publicationHash, neutralValues } from "../src/index.ts";
const publication = await publicationHash(exampleUi);
const identity = { version: 1, clientId: "companion-a", sessionId: "session-a", generation: "1", publicationHash: publication };
const frames = {
  "panel.json": exampleUi,
  "hello.json": { ...identity, type: "hello", hostId: "cockpit-host", companionId: "companion-a", controllerInstanceId: "panel-instance-a" },
  "baseline.json": { ...identity, type: "baseline", sequence: "0", values: neutralValues(exampleUi) },
  "input.json": { ...exampleInputBatch, publicationHash: publication },
  "heartbeat.json": { ...identity, type: "heartbeat", sequence: "1" },
  "feedback.json": { ...identity, type: "feedback", sequence: "1", values: [{ channelId: "engine", value: "Running", provenance: "game-confirmed", sourceKind: "simulator", sourceAgeMs: 0, unit: "status" }] },
};
for (const [name, value] of Object.entries(frames)) await writeFile(new URL(name, import.meta.url), JSON.stringify(value, null, 2) + "\n");
