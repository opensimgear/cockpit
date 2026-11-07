const DECODE_SPACING_MS = 200;

/** Decode only fresh camera frames, with at most one decode in flight. */
export function scanQrFrames(
  video: HTMLVideoElement,
  decode: () => Promise<string | undefined>,
  found: (code: string) => void,
  failed: (reason: unknown) => void,
): () => void {
  let active = true;
  let pending: number | undefined;
  let nextDecode = 0;
  const schedule = () => {
    pending = video.requestVideoFrameCallback(now => {
      pending = undefined;
      if (!active) return;
      if (now < nextDecode) { schedule(); return; }
      nextDecode = now + DECODE_SPACING_MS;
      void read();
    });
  };
  async function read() {
    try {
      const code = await decode();
      if (!active) return;
      if (code === undefined) schedule();
      else { active = false; found(code); }
    } catch (reason) {
      if (!active) return;
      active = false;
      failed(reason);
    }
  }
  schedule();
  return () => {
    active = false;
    if (pending !== undefined) video.cancelVideoFrameCallback(pending);
    pending = undefined;
  };
}
