import { canonical, type Publication } from '../../packages/companion-designer/src/index.ts';

export type CompanionCommand = 'companion_feedback' | 'companion_status' | 'companion_enable' | 'companion_disable' | 'companion_pair_offer' | 'companion_approve' | 'companion_revoke' | 'companion_assign' | 'companion_disconnect' | 'companion_takeover' | 'companion_publish_begin' | 'companion_publish_chunk' | 'companion_publish_commit' | 'companion_publish_abort';
export interface CompanionHostState {
  listener?: { enabled: boolean; endpoint?: string | null; caSpkiSha256?: string; lastError?: string | null; discovery?: unknown; interfaces?: string[] };
  host?: { hostId?: string; clients?: Record<string, unknown>[]; pending?: Record<string, unknown>[]; instances?: Record<string, unknown>[]; publications?: Record<string, unknown>[] };
  pairing?: { manualPayload: string; qrMatrix: boolean[][]; ttlMs: number };
  interfaces?: string[];
  uploadId?: string;
}
export type CompanionSend = (command: CompanionCommand, payload?: Record<string, unknown>) => Promise<CompanionHostState>;

/** Local IPC upload only; this never connects a peer or arms any output. */
export async function sendPublication(publication: Publication, send: CompanionSend): Promise<CompanionHostState> {
  const bytes = new TextEncoder().encode(JSON.stringify({ canonicalDocument: canonical(publication.document), publicationHash: publication.publicationHash, assets: publication.assets }));
  if (bytes.length > 13 * 1024 * 1024) throw new Error('Published panel exceeds the native upload budget.');
  const result = await send('companion_publish_begin', { size: bytes.length });
  if (!result.uploadId) throw new Error('The native panel upload did not start.');
  try {
    for (let offset = 0; offset < bytes.length; offset += 8192) await send('companion_publish_chunk', { uploadId: result.uploadId, offset, bytes: Array.from(bytes.subarray(offset, offset + 8192)) });
    return await send('companion_publish_commit', { uploadId: result.uploadId });
  } catch (error) {
    await send('companion_publish_abort', { uploadId: result.uploadId }).catch(() => {});
    throw error;
  }
}

/** Never infer an authenticated identity from an IP address or display name. */
export function stableCompanionId(value: unknown): string {
  if (typeof value !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value)) throw new Error('Invalid companion identity.');
  return value;
}
