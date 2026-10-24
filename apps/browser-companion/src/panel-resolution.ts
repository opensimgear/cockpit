export {panelResolutionWarning} from '../../../packages/widget-runtime/src/panel-resolution.ts';

/** Presentational telemetry only; it never establishes input or render authority. */
export function panelDisplayReport(publicationHash: string | undefined, pageId: string | undefined, cssWidth: number, cssHeight: number, pixelRatio: number) {
  if (Boolean(publicationHash)!==Boolean(pageId) || ![cssWidth, cssHeight, pixelRatio].every(Number.isFinite)
    || cssWidth <= 0 || cssHeight <= 0 || cssWidth > 16384 || cssHeight > 16384
    || pixelRatio < .25 || pixelRatio > 8 || cssWidth * pixelRatio > 65536 || cssHeight * pixelRatio > 65536) return null;
  return { ...(publicationHash&&pageId?{publicationHash,pageId}:{}), cssWidth, cssHeight, pixelRatio };
}
