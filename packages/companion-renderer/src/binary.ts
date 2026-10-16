import { DIRECTORY_BYTES, HEADER_BYTES, MAGIC, MAX_OBJECT_BYTES, RECORD_BYTES } from "./abi.ts";
import type { BinaryObject, BinarySection, InspectedSection, ObjectKind } from "./types.ts";
import { assert, uint } from "./validation.ts";

const aligned = (value: number) => Math.ceil(value / 4) * 4;
export function view(bytes: Uint8Array): DataView { return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); }
export function section(object: BinaryObject, kind: number): InspectedSection {
  return object.sections.get(kind) ?? { kind, data: new Uint8Array(), offset: 0, recordBytes: RECORD_BYTES[object.kind][kind], count: 0 };
}
export function records(object: BinaryObject, kind: number): DataView[] {
  const table = section(object, kind);
  return Array.from({ length: table.count }, (_, index) => view(table.data.subarray(index * table.recordBytes, (index + 1) * table.recordBytes)));
}
export function zero(data: DataView, start: number, end: number): void {
  for (let i = start; i < end; ++i) assert(data.getUint8(i) === 0, "Nonzero reserved/padding byte");
}
export function boundedRange(offset: number, length: number, total: number, name = "range"): void {
  uint(offset); uint(length); assert(offset <= total && length <= total - offset, `${name}: outside section`);
}
export function inspectBinaryObject(kind: ObjectKind, bytes: Uint8Array, maxBytes = MAX_OBJECT_BYTES): BinaryObject {
  uint(maxBytes, MAX_OBJECT_BYTES, "Object admission budget");
  assert(bytes instanceof Uint8Array && bytes.length >= HEADER_BYTES && bytes.length <= maxBytes, "Object byte size outside budget");
  const data = view(bytes), magic = String.fromCharCode(...bytes.subarray(0, 4));
  assert(magic === MAGIC[kind], "Object magic mismatch");
  assert(data.getUint16(4, true) === 1 && data.getUint16(6, true) === HEADER_BYTES, "Unsupported object version/header");
  assert(data.getUint32(8, true) === bytes.length && data.getUint32(12, true) === HEADER_BYTES, "Object length/directory mismatch");
  const entries = data.getUint16(16, true), layouts = RECORD_BYTES[kind];
  assert(entries > 0 && entries <= Object.keys(layouts).length, "Invalid directory count");
  zero(data, 18, HEADER_BYTES);
  boundedRange(HEADER_BYTES, entries * DIRECTORY_BYTES, bytes.length, "directory");
  const sections = new Map<number, InspectedSection>();
  let cursor = HEADER_BYTES + entries * DIRECTORY_BYTES, previousKind = 0;
  for (let index = 0; index < entries; ++index) {
    const base = HEADER_BYTES + index * DIRECTORY_BYTES;
    const sectionKind = data.getUint16(base, true), recordBytes = data.getUint16(base + 2, true);
    const offset = data.getUint32(base + 4, true), count = data.getUint32(base + 8, true), length = data.getUint32(base + 12, true);
    assert(Object.hasOwn(layouts, sectionKind) && sectionKind > previousKind && recordBytes === layouts[sectionKind], "Unknown, duplicate or unsorted section/layout");
    assert(count > 0 && count <= Math.floor(maxBytes / recordBytes) && count * recordBytes === length, "Invalid section count/length");
    assert(offset === aligned(cursor), "Overlapping, unaligned or noncanonical section offset");
    boundedRange(offset, length, bytes.length, "section"); zero(data, cursor, offset);
    sections.set(sectionKind, { kind: sectionKind, recordBytes, count, offset, data: bytes.subarray(offset, offset + length) });
    cursor = offset + length; previousKind = sectionKind;
  }
  assert(cursor === bytes.length, "Unexpected trailing bytes");
  return { kind, bytes, sections };
}
/** Packs already lowered records. Full semantic and cross-object validation occurs in packPackage. */
export function packBinaryObject(kind: ObjectKind, inputs: readonly BinarySection[]): Uint8Array {
  const sections = [...inputs].filter(input => input.data.length > 0).sort((first, second) => first.kind - second.kind);
  assert(sections.length > 0 && sections.length <= Object.keys(RECORD_BYTES[kind]).length, "Invalid section count");
  let length = HEADER_BYTES + sections.length * DIRECTORY_BYTES;
  for (const input of sections) {
    const stride = RECORD_BYTES[kind][input.kind];
    assert(input.data instanceof Uint8Array && stride && input.data.length % stride === 0, "Invalid section records");
    length = aligned(length) + input.data.length; assert(length <= MAX_OBJECT_BYTES, "Object byte size outside budget");
  }
  const bytes = new Uint8Array(length), data = view(bytes);
  bytes.set(new TextEncoder().encode(MAGIC[kind])); data.setUint16(4, 1, true); data.setUint16(6, HEADER_BYTES, true);
  data.setUint32(8, length, true); data.setUint32(12, HEADER_BYTES, true); data.setUint16(16, sections.length, true);
  let cursor = HEADER_BYTES + sections.length * DIRECTORY_BYTES;
  for (const [index, input] of sections.entries()) {
    const offset = aligned(cursor), base = HEADER_BYTES + index * DIRECTORY_BYTES, stride = RECORD_BYTES[kind][input.kind];
    data.setUint16(base, input.kind, true); data.setUint16(base + 2, stride, true); data.setUint32(base + 4, offset, true);
    data.setUint32(base + 8, input.data.length / stride, true); data.setUint32(base + 12, input.data.length, true);
    bytes.set(input.data, offset); cursor = offset + input.data.length;
  }
  inspectBinaryObject(kind, bytes); return bytes;
}
