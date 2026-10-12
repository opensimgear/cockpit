# macOS gateway simulator

This dependency-free Swift helper creates one CoreHID joystick per simulated module. The descriptors expose packed buttons and unsigned 16-bit X/Y/Z axes. macOS 15 or newer and an Xcode SDK containing CoreHID are required.

```sh
swift build --package-path simulator/macos
swift test --package-path simulator/macos
simulator/macos/.build/debug/osg-gateway-simulator
```

The executable accepts newline-delimited JSON on stdin and responds on stdout. Diagnostics use stderr. Cockpit's local development bridge uses this transport and reports actual CoreHID initialization errors.

macOS restricts virtual HID creation. On the development machine, an ordinary user launch returned `nil` from `HIDVirtualDevice.init`, and an ad-hoc signature containing `com.apple.developer.hid.virtual.device` was rejected by macOS. The default helper is therefore built without this restricted entitlement and returns a clear error when creation is denied. A normally launched app requires an Apple-authorized signing identity and provisioning profile granting the virtual HID entitlement. `virtual-device.entitlements` is the signing template; do not ad-hoc-sign with it because macOS rejects that process.

Apple's published [IOHIDResourceUserClient.cpp](https://github.com/apple-oss-distributions/IOHIDFamily/blob/main/IOHIDFamily/IOHIDResourceUserClient.cpp) checks the virtual HID entitlement without a root exemption. Administrator launch alone therefore does not solve this requirement.

## Signing setup

1. Obtain an Apple signing identity and provisioning profile that authorize [com.apple.developer.hid.virtual.device](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.hid.virtual.device) for your macOS app/helper identity. Arrange access through your Apple Developer team. A signing certificate by itself does not grant the capability; do not substitute an ad-hoc signature or administrator launch.
2. Build this helper with the CoreHID-capable macOS SDK. Package/sign/provision it through your team's macOS app signing workflow, using `virtual-device.entitlements` as the HID entitlement template. Retain any application/team identifier entitlements required by that profile. Ensure the provisioning profile is embedded in the signed app as required by your signing workflow.
3. Verify the exported app/helper before connecting Cockpit:

   ```sh
   codesign --verify --strict --verbose=2 '/absolute/path/to/signed/helper-or-app'
   codesign -d --entitlements :- '/absolute/path/to/signed/helper-or-app'
   ```

   The signature must be accepted by macOS, and the signed entitlements must include the Boolean HID capability. A plist containing the key is not proof that Apple has authorized it. This signing/export step remains untested here because no identity/profile is available.
4. Point the local bridge at the provisioned executable, not the `.app` directory. For an app package use its `Contents/MacOS/osg-gateway-simulator` executable. From the Cockpit repository:

   ```sh
   OSG_HID_HELPER='/absolute/path/to/signed/osg-gateway-simulator' pnpm dev
   ```

   The bridge launches this executable directly over stdin/stdout and skips rebuilding it, preserving its signature. Click **Start gateway**. CoreHID must succeed before Cockpit shows any active HID controllers.
5. In another terminal, independently observe actual OS discovery and input reports:

   ```sh
   pnpm simulator:verify --watch --timeout 20
   ```

   Expect four controllers, including the reserved offline handbrake, with serials `OSG-SIM-OSG-SQ-0042`, `OSG-SIM-OSG-PD-0018`, `OSG-SIM-OSG-BP-0007`, and `OSG-SIM-OSG-HB-0023`. Use Cockpit's **Inputs** controls during the watch interval. A shifter Downshift sends byte `01`; Neutral sends `02`; Upshift sends `04` when other buttons are released and inversion is off. Pedal axes are three unsigned little-endian 16-bit values. Disconnect a module and verify neutral inputs with the same registry ID; register the auxiliary panel and explicitly reconnect to see the fifth controller. Reconnect recreates registry IDs while retaining serial identities.

## Optional socket transport

An external launcher can supply its own private Unix socket and connect an appropriately signed helper to it. Cockpit's Vite bridge uses stdin/stdout rather than creating this socket:

```sh
/absolute/path/to/signed/osg-gateway-simulator --connect-socket /private/tmp/osg-gateway-…/gateway.sock
```

The helper validates the socket's owner against the peer user and restricts its command protocol to the predefined game controllers. Real OS-visible controller creation remains unverified until a provisioned helper is available.

## Simulator protocol

Commands use `{ "id": "request-id", "command": "status" }`, with optional `moduleId`, `buttons`, `axes`, `online`, and `settings`. Commands are `status`, `start`, `stop`, `input`, `register`, `reconnect`, `presence`, and `configure`. Responses contain `id`, `ok`, `state`, and an optional `error`.

- `input` uses held button indices starting at 1 and complete axis arrays in 0…65535. Snapshots retain raw values. Axis inversion is applied to HID reports; shifter inversion exchanges buttons 1 and 3. Digital changes use the configured 0–100 ms debounce window; a newer state supersedes an older pending report, including release events.
- `presence` sends a neutral report when a simulated CAN module is unplugged, while retaining its HID controller slot and serial identity. Offline axes remain neutral even when inverted.
- `register` adds the predefined auxiliary panel as pending. `reconnect` rebuilds controllers and activates its slot. Starting after stopping also creates the registered layout.
- `configure` validates and saves simulator settings. A controller's macOS product label updates on the next reconnect; changes to inversion and debounce apply immediately.
- `stop`, transport disconnect, or process exit removes the virtual controllers.

The experimental vendor/product IDs `1209:f101` through `1209:f105` are local test identifiers, not an assigned OpenSimGear USB identity. This simulates the gateway's HID controllers. It does not enumerate a physical USB composite device, emulate USB CDC or CAN electrical traffic, or claim the still-pending gateway wire protocol is final.
