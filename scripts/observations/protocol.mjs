/** @typedef {Record<string, unknown> & {kind: string, topic: string, source: string, session: string, generation: string, revision: string, capturedUs: string, clockId: string, continuity: string, sequence?: string, reason?: string, payload?: unknown}} ObservationFrame */
/** @typedef {{requestId: string, subscriptionId: string, delivery: string, frame: ObservationFrame}} NativeDelivery */

/** @param {unknown} value @returns {value is string} */
const ascii = value => typeof value === 'string' && /^[\x21-\x7e]{1,128}$/.test(value);
/** @param {unknown} value @returns {value is string} */
export const decimal = value => typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value) && BigInt(value) <= 18446744073709551615n;
export const identity = ascii;

/** @param {unknown} envelope @param {number} bytes @returns {NativeDelivery} */
export function validateDelivery(envelope, bytes) {
  const value = (/** @type {{observation?: Partial<NativeDelivery>} | undefined} */ (envelope))?.observation;
  const frame = value?.frame;
  if (bytes > 1048576 + 4096 || !ascii(value?.requestId) || !ascii(value?.subscriptionId) ||
      !decimal(value?.delivery) || !frame || typeof frame !== 'object' || Array.isArray(frame)) {
    throw new Error('Invalid native observation envelope.');
  }
  const keys = ['kind', 'topic', 'source', 'session', 'generation', 'revision', 'sequence', 'capturedUs', 'clockId', 'continuity', 'payload', 'reason'];
  const kind = frame.kind;
  if (Object.keys(frame).some(key => !keys.includes(key)) ||
      !['snapshot', 'change', 'loss'].includes(kind) ||
      !['topic', 'source', 'session', 'clockId'].every(key => ascii(frame[key])) ||
      !['generation', 'revision', 'capturedUs'].every(key => decimal(frame[key])) ||
      frame.continuity !== (/** @type {Record<string, string>} */ ({ snapshot: 'baseline', change: 'continuous', loss: 'lost' }))[kind] ||
      (kind === 'change' ? !decimal(frame.sequence) : frame.sequence !== undefined) ||
      (kind === 'loss' ? !ascii(frame.reason) || frame.payload !== undefined : !Object.hasOwn(frame, 'payload') || frame.reason !== undefined) ||
      Buffer.byteLength(JSON.stringify(frame)) > (kind === 'change' ? 65536 : 1048576)) {
    throw new Error('Invalid native observation frame.');
  }
  return /** @type {NativeDelivery} */ (value);
}

/** @param {import('node:http').IncomingMessage} req */
export function authorize(req) {
  let origin;
  try { origin = new URL(req.headers.origin ?? 'http://invalid'); } catch { /* denied below */ }
  if (req.method !== 'POST' || req.headers['x-osg-client'] !== '1' || !origin ||
      !['http:', 'https:'].includes(origin.protocol) || !['127.0.0.1', 'localhost'].includes(origin.hostname) ||
      origin.host !== req.headers.host || !/^application\/json(?:\s*;|$)/i.test(req.headers['content-type'] ?? '')) {
    throw new Error('Only the local Cockpit development UI may access observations with JSON.');
  }
}

/** @param {import('node:http').IncomingMessage} req @returns {Promise<Record<string, unknown>>} */
export async function readBody(req) {
  const chunks = [];
  let size = 0;
  for await (const bytes of req) {
    size += bytes.length;
    if (size > 1024) throw new Error('Observation request exceeds its size limit.');
    chunks.push(bytes);
  }
  const value = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid observation request.');
  return value;
}
