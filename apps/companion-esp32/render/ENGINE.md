# Downloadable scene engine

`engine.{h,c}` joins the two-slot raw-flash store, exact canonical manifest admission, the bounded C scene renderer, and existing logical input sessions. It owns one scene and one manifest buffer. No whole package is allocated. The network task initializes JSON hooks before boot scan, supplies chunks of at most 2048 bytes, and performs all JSON parsing. Main renders at most eight tiles per acquired slice. Touch reads only the immutable cache published after a display fence.

The network owner suspends main and waits for its current acquire token to be released before `begin`, `finish`, or `abort`. The `main_suspended` argument asserts that handshake; it is not a mutex inside the engine. Admission temporarily reuses the sole scene. Any rejected candidate re-admits the old pinned view before rendering resumes. A committed package is re-admitted and its logical shim published before input can reconnect. Boot scan restores validated content offline; it never restores a session or armed state.

Manifest validation checks exact canonical bytes and package SHA-256, every immutable store-header field, every object length/hash, source/profile/ABI/viewport identity, required features, all counts, and resource ceilings. Independent C admission verifies records, references, interaction ownership, instructions, drawing limits and lower bounds on memory/work. A correct hash does not authorize semantics: the host must separately compare the trusted publication and bind the open response to source publication, package, profile, and semantic descriptor identities.

| Internal runtime allowance | Sunton | Waveshare |
| --- | ---: | ---: |
| Native engine struct (includes manifest and scene) | 18,648 B | 22,744 B |
| Conservative scene temporary-work allowance | 2,048 B | 2,048 B |
| Adapter, delivery, immutable hit-cache and serial diagnostics reserve | 2,048 B | 2,048 B |
| Admission minimum | 22,744 B | 26,840 B |
| Profile ceiling | 24,576 B | 32,768 B |
| Manifest capacity | 4,096 B | 8,192 B |

Sunton builds define `OSG_RENDER_SUNTON=1`; native verification compiles and exercises both layouts separately. Platform code statically checks its additional state fits the auxiliary reserve. The existing global firmware defaults already provision an 8,192-byte main-task stack on both boards; this change does not add task-stack memory. The table conservatively includes scene temporary work, which runs within that existing stack. Exact target cross-compiler `-fstack-usage` reports app_main=224 B, engine_update=1,184 B, scene_update=1,536 B, expression_evaluate=144 B: the principal update chain is 3,088 B before flash/SDK frames. Rendering and cache publication use separate smaller paths. The known chain leaves about 5 KiB of stack margin; physical high-water measurements are still required for SDK/interrupt effects. Native pointer widths make the table conservative for ESP32; actual firmware heap margins still need board measurements. Baseline runtime/image slots, task stacks, transport buffers and bounded temporary JSON heap are existing ecosystem costs and must also be considered in device-wide heap admission. The manifest ceiling is not permission to consume all remaining free internal memory.

The logical compatibility shim admits at most 32 total controls plus outputs and four pages. Initial controls are momentary buttons, each with exactly one non-overlapping hit. Boolean/number/text output descriptors remain available to the existing feedback/session layer. Placeholder feedback geometry occupies a spare pixel outside buttons, while actual rendering always uses the scene. Source IDs/channels must satisfy the existing logical-ID grammar; legacy display-safe unit text is required. Missing spare space, duplicate IDs, excessive logical widgets, unsupported interactions, and invalid legacy geometry reject explicitly. Navigation hits remain scene-local and cause a stopped reconnect to the requested page. Dynamic text shaping and advanced XY/triangle input contracts remain outside this initial embedded profile.

Main draws the trusted input-health strip only in the reserved top 24 and bottom 8 pixels. Connected/arming/armed changes and a newly published logical panel shim trigger a fence even when scene pixels and package identity are unchanged. Warm reuse therefore acknowledges the new activation without redrawing valid scene content. Display or chrome failure cancels input and clears the published hit cache. Driver waits run without the UI mutex; a scene may be replaced only between acquired slices.

Run from the Cockpit root:

```
python3 apps/companion-esp32/test/engine.py
python3 apps/companion-esp32/test/main_scene.py
python3 apps/companion-esp32/test/run.py
```

ASan/UBSan engine tests stream the real A/B packages for both boards through OpenSSL-backed flash callbacks. They cover exact-manifest and metadata binding, profile/resource rejection, crypto failures, corrupted objects, journal failure rollback, cold/warm scan, feedback freshness, logical-shim bounds, and interaction-index permutation of pressed visuals. Coverage: engine 99.55% lines / 72.28% branches / 100% functions. The actual main scene helper is tested against deterministic platform boundaries for slicing, suspension/replacement, chrome-only fences and display failure (100% / 72.22% / 100%). Real raster tests prove status refresh never modifies package content pixels on either resolution (100% / 87.39% / 100%). These are software checks; physical board, transport, power-cut, and input lease qualification remain separate evidence.
