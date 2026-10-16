import { writeFile } from "node:fs/promises";
import { flightTemplate, racingTemplate, exportBundle } from "../src/index.ts";
await writeFile(new URL("flight-panel.json", import.meta.url), await exportBundle(flightTemplate, []) + "\n");
await writeFile(new URL("racing-dashboard.json", import.meta.url), await exportBundle(racingTemplate, []) + "\n");
