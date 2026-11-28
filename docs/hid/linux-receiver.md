# Linux hidraw reception and qualification

The generic `controller_hid_*` gateway inspects selected Linux hidraw interfaces. It is separate from the mapping-oriented evdev controller projection. evdev key/axis events do not establish original HID descriptor or report-byte coverage. HID descriptor inspection, a selected input stream and an explicit input/feature GET are separate capabilities; none authorizes a report write.

The [Linux kernel hidraw documentation](https://www.kernel.org/doc/html/latest/hid/hidraw.html) specifies descriptor retrieval through `HIDIOCGRDESCSIZE` and `HIDIOCGRDESC`, and unparsed input reports through `read()`. Numbered read reports include their ID; unnumbered reports begin with payload. The qualification probe uses those read-only descriptor/raw-info operations and a nonblocking read handle. It never invokes feature/input GETs, SET_REPORT, output writes, device grabbing or virtual input.

## What the maintained runner checks

[`verify-linux-receiver.mjs`](../../tests/hid-device-captures/verify-linux-receiver.mjs) connects to the **already running native Linux Cockpit WebKitGTK WebView** through the existing [WebKit inspector adapter](../../scripts/reactive/webkit.mjs). It calls native `gateway_request`, rather than Chromium CDP or a separately spawned browser development service. The selected path must match exactly one inventory row. There is no product-name, VID/PID or first-device fallback.

The runner compares three independently observable boundaries:

1. [`verify-linux-probe.py`](../../tests/hid-device-captures/verify-linux-probe.py) opens only the explicitly supplied `/dev/hidrawN`, records original descriptor bytes via the Linux ioctl ABI and original stream deliveries via `read()`, then closes the handle. Descriptor/identity are checked again after capture. Symlinks are refused. This helper has no Cockpit or portable parser dependency.
2. The running Cockpit supplies its current generation's inspection descriptor, original stream `wireHex`, sequence and serialized H1 decoded samples. The descriptor must exactly match the independent ioctl bytes. A native gap, terminal status, changed descriptor, stale generation, permission failure or incomplete capture fails acceptance. Profiles must already be stopped; the runner never stops profiles or changes configuration.
3. [`qualify.rs`](../../crates/hid-report/examples/qualify.rs) is a verification-only portable decoder executable. It receives bounded JSON on stdin, performs no OS/device access and returns the same parser/decoder model. Up to 64 native reports whose exact bytes also occurred in the independent window are compared, including framing, raw bits, exact decimal values, usages, null/invalid states and physical metadata. This is cross-entry parity with the shared portable implementation, **not** an independent decoder algorithm.

The two live readers overlap but are not packet-synchronized. Identical bytes in both windows establish exact wire-byte overlap, not that timestamps identify the same physical packet. Relative repeats are retained as separate sequence entries. No cross-clock timestamp alignment or device sampling time is inferred.

## Prepare on the Linux host

Use an x86_64 or aarch64 Linux host for the independent probe's explicitly supported asm-generic ioctl encoding. This restriction applies to the qualification probe, not a claim about every library target. Python 3 and Node 22 or newer are needed. Install the project's ordinary Linux native build dependencies before building Cockpit. Run the application and probe as the same normal user with access to the selected node. Permission denial remains an error; this runner installs no udev rules, changes no device permissions and invokes no privilege helper.

Build the offline adapter and run its fixture/fake-kernel checks first:

```sh
cargo build --locked --manifest-path crates/hid-report/Cargo.toml --example qualify
cargo test --locked --manifest-path crates/hid-report/Cargo.toml --example qualify
node --test tests/hid-device-captures/verify-linux.test.mjs
python3 tests/hid-device-captures/verify-linux-probe-tests.py
```

These commands do not open a device or contact Cockpit. The Python tests inject a fake kernel boundary; their generated objects are never archived as hardware evidence. The Rust adapter also exercises all 18 original authored golden reports.

Start the native development Cockpit yourself with its private WebKit inspector, using the ordinary project development command:

```sh
WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:9285 pnpm dev
```

Inspect the current inventory and select the intended interface explicitly. A composite device can expose several hidraw nodes and evdev interfaces; an event node is not a hidraw substitute. A node number is a current address, not durable physical identity. Keyboard/mouse/system restrictions, virtual-device exclusion, OS permission checks and generation fences remain native admission decisions. An inaccessible or metadata-only row is valid inventory evidence and does not become a qualified stream.

If Linux has no passed-through hidraw nodes, verify only the real native inventory path:

```sh
node tests/hid-device-captures/verify-linux-receiver.mjs \
  --inventory-only \
  --webkit http://127.0.0.1:9285/ \
  --output "$PWD/tests/hid-device-captures/.generated/linux-inventory-001"
```

This mode requires no decoder executable, Python probe, selected device or hardware-read flag. It makes one actual native inventory request and records its rows, failures and truncation state in `linux-hidraw-inventory.json`. Zero interfaces is an explicit valid observation. Both console and receipt say `hardwareAcceptance:"not-run"` and `deviceOpenedByRunner:false`; a successful inventory request is not device reception qualification. The runner does not bind/pass through USB devices, request UAC or create synthetic hidraw nodes.

With that native app already running, perform the opt-in read:

```sh
node tests/hid-device-captures/verify-linux-receiver.mjs \
  --device /dev/hidraw7 \
  --webkit http://127.0.0.1:9285/ \
  --duration-ms 2000 \
  --output "$PWD/tests/hid-device-captures/.generated/linux-candidate-001" \
  --allow-hardware-read
```

Replace `/dev/hidraw7` with the exact selected current path. `--decoder` can specify the absolute verification executable when using a custom Cargo target directory; `--python` can select the Python executable. Neither the runner nor its tests launch Cockpit, install drivers, reconnect devices or actuate controls. Both opened readers are closed, including on assertion failure. The independent child is terminated if qualification aborts before it finishes. A stuck kernel operation can outlive process cancellation; no successful receipt is issued for that failure.

The native public response contract used by this runner is:

| Command | Required response |
| --- | --- |
| `controller_hid_list` | `state.controllers.interfaces[]` with exact `path` and opaque `interfaceId`; explicit global failures/truncation |
| `controller_hid_inspect` | Selected `interfaceId`, original `descriptorHex` and serialized H1 `descriptor`; unavailable metadata remains explicit |
| `controller_hid_open` | Selected `interfaceId` plus fresh `generation` |
| `controller_hid_reports` | Same target/generation; `result.reports[]` with decimal `sequence`, `wireHex`, exact H1 `samples`, plus `gap` and `terminal` |
| `controller_hid_close` | Closure of that exact target/generation; runner waits for success before writing its receipt |

No explicit GET is sent by this runner. Feature/input GET admission, short responses, deadlines, cancellation, denied access and removal need their native boundary tests and a separately requested hardware procedure. A stream pass cannot qualify those operations.

## Bounds and interpretation of the receipt

Each reader retains at most 4,096 reports or 8 MiB. The independent helper accepts at most 4,096 descriptor bytes (the Linux hidraw descriptor ioctl ABI), 65,536 bytes/report and a 100–10,000 ms requested capture window. A capacity stop is an explicit incomplete prefix and fails the acceptance run. The portable adapter accepts at most 512 report requests, 8 MiB aggregate wire data, 65,536 decoded values and bounded JSON input/output; this runner requests at most 64 comparisons. Busy devices may require a shorter window or a separate measured load test, rather than discarding loss indicators.

The private candidate `linux-hidraw-receiver.json` is created with exclusive-create semantics and owner-only permissions; existing receipts are never replaced. It records the original independent descriptor/reports, native retained reports, compared sequence subset, decoder executable SHA-256, OS/architecture and limits. It includes device-specific descriptor content and the selected local path. Review/redact a candidate before adding physical evidence to the archived corpus.

The receipt always says `releaseQualified:false`. It does not establish signed packages, physical transitions, axis endpoints, hat directions, output/force feedback, all report IDs/kinds, permission-denied behavior, removal/reconnect, or every Linux distribution. Byte changes can be telemetry/counters; they are not proof of physical actuation. An idle device can pass reception/parity while remaining idle-only evidence. No report means no reception qualification. The runner records that the running binary's source revision is not established by this procedure; rebuilding and recording the release artifact remains separate evidence.

## CI and current evidence status

The current `controllers` job in [ci-native.yml](../../.github/workflows/ci-native.yml) includes Ubuntu 24.04 and installs `libudev-dev`, `libdbus-1-dev`, bubblewrap and the distro AppArmor profile before native controller coverage. Those are automated software checks. Its desktop compilation/regression steps explicitly exclude Linux, so that job does not prove a running Linux WebKitGTK HID acceptance test. The existing [archived device corpus](../../tests/hid-device-captures/README.md) contains Windows idle evidence; it must not be relabeled Linux evidence.

For #181, the amd64 debug Debian package was built in Ubuntu on WSL2. The executable extracted from that package served a native inventory request through its running WebKitGTK cockpit. The gateway returned zero interfaces and an enumeration error because the kernel exposed no hidraw class. The controller suite exercised numbered and unnumbered reception, GET handling, permission and generation checks, and a 512-report relative-motion and button sequence using fixtures.

On 2026-11-28 the Linux development cockpit opened a physical VKB T-Rudder (USB 231d:011f) through WSL2 USB passthrough. The independent hidraw reader and cockpit obtained the same 287-byte descriptor. The cockpit retained 18 input reports and the independent reader retained 15 during overlapping capture windows. Both saw one distinct 64-byte report with ID 1. All 18 cockpit projections matched the portable decoder. The device was idle throughout the test. Physical actuation and hardware GET remain unqualified. The [Linux capture sample](../../tests/hid-device-captures/linux-vkb-t-rudder-idle.json) records the original descriptor, input bytes and comparison counts.
