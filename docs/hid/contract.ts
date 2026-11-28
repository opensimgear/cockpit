/** H0 wire contract for H1–H9. These types describe data, not operating-system handles. */
export type Decimal = string; // 0 or -?[1-9][0-9]*; unsigned fields additionally reject '-'
export type Hex = string; // whole bytes, two lowercase digits per byte, in transport order
export type Digest = string; // sha256: followed by 64 lowercase hex digits
export type ReportKind = 'input' | 'output' | 'feature';
export type EvidenceSource = 'report-descriptor' | 'windows-hidp' | 'windows-raw-input' |
  'windows-hid-read' | 'linux-hidraw' | 'macos-element' | 'macos-report' | 'vjoy-sdk';
export type Evidence<T> =
  | { state: 'observed'; source: EvidenceSource; value: T }
  | { state: 'unavailable'; reason: 'not-exposed' | 'access-denied' | 'not-declared' | 'unsupported' | 'malformed' };

export interface Target {
  /** Opaque backend interface instance, never just VID/PID, name, usage or container. */
  interfaceId: string;
  /** Within this interface's snapshot; null addresses the interface itself. */
  collectionId: string | null;
  /** New token on service startup and each open/re-enumeration or descriptor replacement. */
  generation: string;
  descriptorDigest: Digest;
}

export interface Usage { page: number; id: number }
export type LocalIndex<T> = { value: T } | { minimum: T; maximum: T };
export interface Locals {
  usages: LocalIndex<Usage>[];
  strings: LocalIndex<number>[];
  designators: LocalIndex<number>[];
}
export interface Collection {
  id: string;
  parentId: string | null;
  typeCode: Evidence<number>;
  usage: Evidence<Usage>;
  /** Index of the declaring Main item in the byte-order item list, when available. */
  itemIndex: Evidence<number>;
}
export interface Item {
  offsetBytes: number;
  sizeBytes: number; // includes header and any long-item size/tag bytes
  form: 'short' | 'long';
  typeCode: number;
  tag: number;
  disposition: 'interpreted' | 'preserved' | 'malformed';
}
export interface Field {
  id: string; // stable within this snapshot; usages are not unique identifiers
  collectionId: Evidence<string>;
  itemIndex: Evidence<number>;
  reportType: ReportKind;
  reportId: number | null; // null denotes an unnumbered report
  offsetBits: Evidence<number>; // payload-relative, LSB first
  sizeBits: Evidence<number>;
  count: Evidence<number>;
  flags: Evidence<Decimal>; // complete unsigned Main item payload, including reserved bits
  /** Partial OS projections can expose individual flags while the complete mask is unavailable. */
  flagEvidence: Partial<Record<'constant' | 'variable' | 'relative' | 'wrap' | 'nonlinear' |
    'noPreferred' | 'nullState' | 'volatile' | 'bufferedBytes', Evidence<boolean>>>;
  logical: { minimum: Evidence<Decimal>; maximum: Evidence<Decimal> };
  physical: { minimum: Evidence<Decimal>; maximum: Evidence<Decimal> };
  unitCode: Evidence<Decimal>;
  unitExponent: Evidence<number>;
  primary: Evidence<Locals>;
  alternatives: Evidence<Locals[]>; // ordered delimiter branches, including empty branches
  aliases: Evidence<Usage[]>;
}
export type Access =
  | { state: 'available' }
  | { state: 'unavailable'; reason: 'unsupported' | 'not-advertised' | 'permission-denied' |
      'exclusive-owner' | 'qualification-gated' | 'offline'; nativeCode?: string };
export interface Capabilities {
  descriptorBytes: Access;
  parsedCapabilities: Access;
  inputStream: Access;
  inputRead: Access;
  featureRead: Access;
  vjoyEffectObservation: Access;
}
export interface Snapshot {
  target: Target;
  backend: 'windows' | 'linux' | 'macos' | 'vjoy';
  provenance: EvidenceSource;
  capabilities: Capabilities;
  rawDescriptor: Evidence<Hex>;
  items: Evidence<Item[]>;
  collections: Collection[];
  fields: Field[];
  layouts: { kind: ReportKind; reportId: number | null; payloadBits: Evidence<number> }[];
  availability: 'live' | 'last-observed';
}
export type Sample =
  | { state: 'valid'; kind: 'integer'; value: Decimal }
  | { state: 'valid'; kind: 'array-selector'; selector: Decimal; usages: Usage[] }
  | { state: 'valid'; kind: 'bytes'; bytes: Hex }
  | { state: 'null' | 'invalid' | 'unknown'; reason: string; encoded: Decimal | null };
export interface Value {
  fieldId: string;
  elementIndex: number;
  rawBits: Evidence<Hex>; // right-aligned little endian, unused high bits zero
  sample: Sample;
  semantics: 'absolute' | 'relative' | 'event' | 'opaque';
  physical?: { numerator: Decimal; denominator: Decimal; unitCode: Decimal; exponent: number };
}
export interface Report {
  target: Target;
  sequence: Decimal; // unsigned within target generation
  capturedUs: Decimal; // monotonic native acquisition time, local to this capture clock
  deviceTime?: { ticks: Decimal; clockId: string; ticksPerSecond: Decimal };
  source: 'stream-input' | 'explicit-input-read' | 'explicit-feature-read' | 'vjoy-effect';
  kind: ReportKind;
  reportId: number | null;
  /** Original backend delivery, including any framing byte or trailing padding it delivered. */
  bytes: Evidence<Hex>;
  framing: 'id-prefix' | 'unnumbered' | 'os-zero-prefix' | 'id-out-of-band' | 'element-only' | 'sdk-packet';
  values: Value[];
  diagnostics: string[];
}
export type CaptureRecord =
  | { type: 'report'; report: Report }
  | { type: 'gap'; target: Target; firstSequence: Decimal; lastSequence: Decimal; reason: string }
  | { type: 'upstream-loss'; target: Target; capturedUs: Decimal; reason: string }
  | { type: 'status'; target: Target; capturedUs: Decimal; state: 'waiting' | 'stale' | 'offline' | 'ended'; reason: string };

export interface ReadRequest {
  requestId: string;
  target: Target;
  kind: 'input' | 'feature';
  reportId: number | null;
  maxBytes: number;
  timeoutMs: number;
}
export type ReadResult =
  | { requestId: string; state: 'received'; report: Report }
  | { requestId: string; state: 'failed'; reason: 'unsupported' | 'denied' | 'busy' |
      'timeout' | 'offline' | 'stale-generation' | 'invalid-layout' | 'malformed-response' |
      'capacity' | 'cancelled' | 'io-error'; nativeCode?: string };
export interface Recording {
  captureId: string;
  origin: 'native-capture' | 'authored';
  clock: { id: string; unit: 'microsecond'; origin: 'native-monotonic' };
  snapshots: Snapshot[];
  records: CaptureRecord[];
  completed: 'stopped' | 'capacity' | 'disconnected' | 'failed';
  truncated: boolean;
}
