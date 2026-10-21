# Portable widget fonts

The `sans` face is unmodified Rajdhani SemiBold from render-package commit
`f2c9147b863a1aab7de1dd9026bf54e5430b16c4`; its source is Google Fonts Rajdhani.
The `mono` face is unmodified IBM Plex Mono Regular from Google Fonts commit
`633f3200539c52ee0aba2dfd7f46921417a81877`, path
`ofl/ibmplexmono/IBMPlexMono-Regular.ttf`.

The SIL Open Font License and copyright notices are retained in the two OFL
files. Bundle these notices with the application/firmware. Font files must not
be sold by themselves. The CSS family aliases do not modify the font files.

`scripts/prepare-widget-fonts.py --check` verifies pinned digests and reproduces
`metrics.json` from TrueType tables without third-party Python dependencies.
Widget text uses unkerned integer advances in its authored coordinate system,
then scales with the widget. Native glyph rasterization may differ by one pixel;
positions, baseline and line clipping use the shared metrics. Complex-script
shaping is outside this simple-glyph widget typography contract. Browser
fallback characters remain visible; device admission must report missing glyphs.
