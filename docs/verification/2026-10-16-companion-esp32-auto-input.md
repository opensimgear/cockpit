# Automatic ESP32 touch activation

The network task activates touch automatically for each connected live panel after display rendering and released-touch health checks. It reuses the existing session/generation/publication/epoch-bound neutral-baseline handshake and acknowledgement; no host lease, input ordering, publication validation or failure handling is bypassed.

Reconnect and assignment refresh produce fresh sessions and repeat the checks. Held touch delays activation until release. Touch during a baseline or a lost/invalid baseline acknowledgement closes the session without retrying that baseline. Pause touch input leaves the session stopped until Reconnect panel.

The wizard no longer requires an Arm button. Automatic activation runs in firmware for both Wi-Fi and direct USB, so Wi-Fi activation after restart does not require a USB connection.

Native protocol/client tests cover missing render, held touch, release-triggered activation, exactly one baseline, pause, failed acknowledgement, touch during baseline and automatic activation after an assignment refresh, in addition to the existing input lease/fault tests.
