# Waveshare framebuffer coherence — 16 October 2026

Milestone 0 of [downloadable companion rendering](../companion-render-packages.md): implementation and native regression are complete. Physical scanout/performance qualification remains pending.

## Change

The LCD7 adapter accepts partial redraws into its inactive RGB565 framebuffer. Swapping buffers without repairing the recycled buffer lost unchanged pixels on the next partial redraw. After a successful presentation and the existing three-new-VSYNC fence, the adapter now copies the complete presented frame into the recycled frame before exposing it for writes. Both buffers therefore start each redraw with the same image.

The copy is exactly 768000 bytes and adds no framebuffer or retained scratch allocation. The existing 80 ms per-fence timeout, failure latch, synchronous caller-buffer consumption and conservative presentation fence remain. Submission failure or any fence timeout leaves both buffers untouched by presentation, retains the previous back-buffer index and prevents subsequent writes/presents until reinitialization. No copy occurs before the final fence completes.

## Verification

Run from `cockpit`:

```text
python3 apps/companion-esp32/test/waveshare.py
```

Passed with GCC warnings as errors, AddressSanitizer, UndefinedBehaviorSanitizer and the existing production coverage gate. Adapter coverage is 100% lines, 86.52% branches and 100% functions. Local evidence is written to `.research/esp32-waveshare-native/{board-test.log,coverage.json}`.

The SDK-mocked test covers:

- Twelve alternating updates to distant regions, comparing every pixel of both frames with the accumulated expected image after each presentation.
- A presentation without further drawing and immediate caller-scratch reuse.
- No framebuffer writes at submission or during any of the three blocking fence waits; queued pre-submission VSYNC is drained.
- Submission failure and independently injected timeout at waits one, two and three: no buffer copy/recycle and no later writes/presents after the fault latch.
- Existing initialization cleanup, geometry, span bounds and touch-report regressions.

A temporary negative-control build using the original swap-only presentation fails the new full-frame coherence assertion. The temporary build does not modify production files.

The Waveshare profile also cross-builds successfully using the existing pinned ESP-IDF v5.5.2 toolchain:

```text
bash apps/companion-esp32/tools/idf.sh -B build-waveshare-lcd7 -DIDF_TARGET=esp32s3 -DSDKCONFIG=build-waveshare-lcd7/sdkconfig -DOSG_BOARD_PROFILE=waveshare-lcd7-s3 build
```

Application image: 894032 bytes (`0xda450`), with 42% of the existing application partition free. This is the current integrated checkout size, not an isolated size delta for the copy. The compile confirms target/API compatibility and does not establish physical scanout behavior.

## Physical gate

These tests model SDK calls and buffer contents, not DMA or physical VSYNC timing. No board was flashed or measured for this checkpoint. On the Waveshare board, alternate disjoint visible regions under transport load, check that all earlier changes persist, and measure full-copy/present duration plus scanout stability and peak memory. Keep the conservative fence until that evidence justifies a change. Record observed results separately from these mock results; the added PSRAM copy has not yet been qualified for latency/bandwidth.
