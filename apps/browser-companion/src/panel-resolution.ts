export interface PanelResolution { width: number; height: number }

/** Compare authored pixels with the physical pixels used by the current display. */
export function panelResolutionWarning(panel: PanelResolution | undefined, cssWidth: number, cssHeight: number, pixelRatio: number): string {
  if (!panel || ![cssWidth, cssHeight, pixelRatio].every(value => Number.isFinite(value) && value > 0)) return '';
  // Ignore fractional layout rounding below one physical pixel.
  const width = Math.ceil(cssWidth * pixelRatio - .01), height = Math.ceil(cssHeight * pixelRatio - .01);
  if (panel.width >= width && panel.height >= height) return '';
  return `The selected panel is too small for this display: ${panel.width} × ${panel.height} px, displayed at ${width} × ${height} px. Widgets may look blurry. Increase the panel size in Cockpit to at least ${width} × ${height} px and recompile, or use a smaller browser window.`;
}
