import { MAX_LINE_BYTES } from '../protocol.ts';

/** Bounded incremental UTF-8 decoder. Delivery callbacks must never await ACK. */
export async function readObservationLines(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  signal: AbortSignal,
  receive: (value: unknown) => void,
): Promise<void> {
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let line = '';
  let lineBytes = 0;
  while (!signal.aborted) {
    const { value, done } = await reader.read();
    if (signal.aborted) return;
    if (done) throw new Error(lineBytes ? 'Truncated observation delivery' : 'Observation stream ended');
    let start = 0;
    for (let index = 0; index <= value.length; index++) {
      if (index < value.length && value[index] !== 10) continue;
      const part = value.subarray(start, index);
      lineBytes += part.length;
      if (lineBytes > MAX_LINE_BYTES) throw new Error('Observation line exceeds byte budget');
      line += decoder.decode(part, { stream: index === value.length });
      if (index < value.length) {
        receive(JSON.parse(line));
        if (signal.aborted) return;
        line = '';
        lineBytes = 0;
      }
      start = index + 1;
    }
  }
}
