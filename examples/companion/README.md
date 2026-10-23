# Generic Toggle

[generic-toggle.source.json](generic-toggle.source.json) is the editable Toggle pilot for the generic widget designer. It contains the scene, typed signals, independent state regions, interaction events, optional motion and sound declarations. It contains no host addresses, credentials, device assignments or simulator bindings.

1. Open **Companions → Widgets → Import** and choose the source file. Open **Toggle** to edit it.
2. Use **Design**, **Data**, **Behavior**, **Motion** and **Sound** to configure the widget. Choose its target dimensions and optional features, then **Compile**.
3. In **Try**, click the widget to change its requested value immediately. Send separate confirmation samples and freshness states from **Simulate**. Inspect the resulting signals, regions, outputs and compiled resource report.
4. Save the widget revision. Create a panel, add Toggle, position and size it, then **Compile** and **Try** the complete panel.
5. Pair a browser companion, choose it for the panel and **Save**. Configure any game control and confirmation routes explicitly in Profiles.

Compilation turns the authoring layers into finished raster tiles, sampled frames and optional baked motion sequences. The deployed panel contains those assets plus bounded behavior and media metadata. Devices select finished images; they do not reconstruct authoring layers. A changed source, size or capability profile requires a fresh compilation.

Requested state and game confirmation are separate. Unknown, stale and disconnected samples have explicit visual presentations. Reduced motion and unavailable audio preserve the widget's state and meaning. Browser audio also requires an explicit user gesture. ESP32 deployment is currently disabled and tracked separately.

The product library starts with Toggle. Linear gauge, radial gauge and three-state sources in the compiler tests exercise generic behavior and sampled rendering without adding product presets.

Run `pnpm widgets:check` and `pnpm widgets:test` from the repository root to validate the source/compiler/runtime contracts. Historical panel and widget bundles now live under [test-only legacy fixtures](../../tests/fixtures/legacy-companion/README.md); current import and publication APIs reject those retired formats.
