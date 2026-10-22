import { canonical, type Publication } from '../../packages/companion-designer/src/index.ts';
import type { RenderProfile } from '../../packages/companion-designer/src/render-profiles.ts';

export type CompanionCommand = 'companion_firmware_check' | 'companion_usb_update_prepare' | 'companion_browser_certificate' | 'companion_update' | 'companion_pair_cancel' | 'companion_usb_flash' | 'companion_prune' | 'companion_unassign' | 'companion_usb_ports' | 'companion_usb_status' | 'companion_usb_connect' | 'companion_usb_disconnect' | 'companion_usb_scan' | 'companion_usb_provision' | 'companion_usb_reconnect' | 'companion_usb_arm' | 'companion_usb_disarm' | 'companion_usb_forget' | 'companion_status' | 'companion_enable' | 'companion_disable' | 'companion_pair_offer' | 'companion_approve' | 'companion_revoke' | 'companion_assign' | 'companion_disconnect' | 'companion_takeover' | 'companion_publish_begin' | 'companion_publish_chunk' | 'companion_publish_commit' | 'companion_publish_abort';
export interface CompanionOperation {
  instanceId:string; publicationHash:string; sessionId?:string; generation?:string; attemptId?:number;
  phase:'opening'|'checking-cache'|'downloading'|'rendering'|'awaiting-neutral'|'ready'|'failed';
  completedBytes?:number; totalBytes?:number; cacheHits?:number; cacheMisses?:number;
  errorCode?:string; retryable?:boolean; message?:string;
  diagnostic?:{widget:string;definition:string;layer:string;field:string;required:number;available:number};
}
export interface CompanionHostState {
  panelSaved?: {panelId:string;publicationHash:string};
  panelSaveApplication?: {state:'inactive'|'applied'|'failed';error:string|null};
  firmwareVersion?: string | null;
  firmwareSource?: 'development' | 'github-release';
  firmwareChecked?: boolean;
  firmwareError?: string | null;
  firmware?: Record<string,{version:string;profile?:string;connectionMode?:string;renderProfile?:RenderProfile|null}>;
  browserUrl?: string;
  firmwareProfiles?: {id:string;name:string;available:boolean;version?:string|null;source?:'development'|'github-release';error?:string|null}[];
  usbPorts?: {port:string;name:string;vid:number;pid:number}[];
  usb?: {devices?: Omit<NonNullable<CompanionHostState['usb']>,'devices'>[];scanning?:boolean;scanError?:string;lastHostError?:string;flashing?:boolean;flashFinished?:boolean;flashError?:string;flashWarning?:string;progress?:number;connected:boolean;ready?:boolean;port?:string;stage?:string;deviceStatus?:string;recovery?:{phase:'waiting'|'connecting'|'waiting-for-device'|'recovered';attempt:number;nextRetryMs:number;message:string};networks?:{ssid:string;rssi:number;secured:boolean}[];device?:{name?:string;ssid?:string;wifiPasswordSaved?:boolean;firmwareVersion?:string;boardProfile?:string;renderProfile?:RenderProfile;status?:string;statusMessage?:string;setupSaved?:boolean;connectionMode?:string;companionId?:string;readyForArm?:boolean;touchAvailable?:boolean;operation?:CompanionOperation}};

  listener?: { configuredEnabled?:boolean;config?:{bind:string;advertisedHost:string;discovery:boolean}|null;enabled: boolean; endpoint?: string | null; caSpkiSha256?: string; lastError?: string | null; discovery?: unknown; interfaces?: string[] };
  host?: { hostId?: string; clients?: Record<string, unknown>[]; pending?: Record<string, unknown>[]; instances?: Record<string, unknown>[]; publications?: Record<string, unknown>[] };
  pairing?: { browserUrl?: string; manualPayload: string; qrMatrix: string[]; ttlMs: number; code?: string };
  interfaces?: string[];
  uploadId?: string;
}
export type CompanionSend = (command: CompanionCommand, payload?: Record<string, unknown>) => Promise<CompanionHostState>;

/** Committing a saved panel lets native code refresh profiles that use it. */
export async function sendPublication(publication: Publication, send: CompanionSend, options:{saveAsLatest?:boolean}={}): Promise<CompanionHostState> {
  const bytes = new TextEncoder().encode(JSON.stringify({ canonicalDocument: canonical(publication.document), publicationHash: publication.publicationHash, assets: publication.assets }));
  if (bytes.length > 13 * 1024 * 1024) throw new Error('Published panel exceeds the native upload budget.');
  const result = await send('companion_publish_begin', { size: bytes.length });
  if (!result.uploadId) throw new Error('The native panel upload did not start.');
  try {
    for (let offset = 0; offset < bytes.length; offset += 8192) await send('companion_publish_chunk', { uploadId: result.uploadId, offset, bytes: Array.from(bytes.subarray(offset, offset + 8192)) });
    return await send('companion_publish_commit', { uploadId: result.uploadId, ...(options.saveAsLatest===false?{saveAsLatest:false}:{}) });
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
