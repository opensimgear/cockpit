# Simultaneous gateway acceptance — 2026-10-13

Cockpit now keeps an independent native connection for every compatible gateway.
The Gateways view lists each gateway and its own devices, Devices aggregates all
inventories, and Topology shows separate chains. Gateway identity scopes requests,
claims, configuration caches, drafts, live inputs, errors and reconnects. Separate
gateways can expose the same module identity without sharing state. The desktop
and browser service use the same manager; browser replies are correlated so one
slow gateway does not serialize requests to another.

## Automated verification

- 51 Rust tests passed, including same-backend and mixed-backend routing,
  handshake identity validation before mutation, independent workers, errors and
  shutdown. Delayed discovery cannot create a worker after shutdown.
- 39 frontend/transport/bridge tests passed, covering independent polling and
  queues, scoped claims/configuration, retry backoff, offline state, cross-scoped
  response rejection and out-of-order browser replies.
- Svelte checking and the official autofixer completed without errors, warnings
  or suggestions. The production frontend and native macOS debug bundle built.
- Real service EOF checks cancelled pending requests in 19–37 ms without stopping
  gateway processes, including EOF during initial connection.

Both initiating repositories ran their own sanitized dependency bootstraps with
explicit snapshots of unpublished source changes:

```sh
# From cockpit/
python3 tests/e2e/run.py --sanitize \
  --source device-gateway=/absolute/path/to/device-gateway \
  --source device-template=/absolute/path/to/device-template

# From device-gateway/
python3 tests/e2e/run.py --sanitize --with-cockpit \
  --source device-template=/absolute/path/to/device-template \
  --source cockpit=/absolute/path/to/cockpit
```

Both passed the original three-device acceptance and the new two-gateway case.
The gateway runner also passed its direct CDC/CAN acceptance. The new case gives
two gateway buses the same module identity, then verifies independent saved
configuration, writer claims, layouts and inputs. It pauses one gateway while the
other answers a concurrent request, checks a scoped claim rejection, disconnects
and reconnects one session, and crashes/restarts one gateway while the peer remains
writable. Dependencies, builds and evidence stay inside each repository's `.e2e/`.

Local passing evidence:

- Cockpit: `.e2e/runs/cockpit-xipTJb/` and
  `.e2e/runs/cockpit-multi-gateway-U0rI5H/`.
- Device gateway: `.e2e/runs/gateway-2y7n0ujt/`,
  `.e2e/runs/cockpit-PInEoZ/` and `.e2e/runs/cockpit-multi-gateway-NGh2yF/`.

The native app/dev watcher was closed during service tests: an automatically
connecting second Cockpit process otherwise competes for a gateway's single CDC
client connection, including during layout reattachment.

## Native application verification

The native app was started before two new independent CAN gateway processes and
two JSON-configured device processes. Together with an existing physical Pico
and an existing independent CAN gateway, Cockpit showed **four connected gateways**
and seven modules. Each gateway had its own card. The two new devices deliberately
shared a module identity on their separate buses.

Through the native UI, both test devices were registered and their layouts applied
independently. The first device was renamed to `Saved on first gateway` and saved
as generation 2; the peer remained `Multi UI pedals 2` at generation 1. Stopping
the first gateway retained its offline controller slot while the second device
continued showing Axis 1 at 100%. Restarting the first gateway on a new endpoint
restored its saved name and registration, with all four gateways connected again.
Devices showed all inventories and Topology showed four separate chains.

Evidence is recorded in `.e2e/runs/ui-k1mvdfm1/result.json`. Temporary test processes
were cleaned up. The updated native app was reopened showing the user's original
two running gateways; their independent processes were preserved.

Native streams exercise production configuration, CAN, registry, descriptors and
reports. They do not create physical USB/HID devices. Physical STM32 USB enumeration
and electrical CAN acceptance remain hardware work. No release version was created.

## Shared computer topology follow-up

Topology now renders one computer with a shared connection line branching to every
gateway. Each branch retains its gateway identity/status and a wrapping grid of
its devices. Offline devices remain visible and selectable; empty gateways show
an explicit inventory message. Gateway regions and device buttons include gateway
identity in their accessible names.

Svelte checking/autofixer and the native bundle build passed. The rebuilt native
app visibly showed one computer, both running gateways and their five modules.
The app was left open on Topology.
