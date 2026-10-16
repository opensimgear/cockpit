export function assert(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
export function exact(value: unknown, keys: readonly string[], name = "object"): Record<string, unknown> {
  assert(value !== null && typeof value === "object" && !Array.isArray(value), `${name}: expected object`);
  const item = value as Record<string, unknown>;
  assert(keys.every(key => Object.hasOwn(item, key)) && Object.keys(item).every(key => keys.includes(key)), `${name}: missing or unknown fields`);
  return item;
}
export function uint(value: unknown, max = 0xffffffff, name = "integer"): asserts value is number {
  assert(typeof value === "number" && Number.isSafeInteger(value) && value >= 0 && value <= max, `${name}: outside unsigned integer bounds`);
}
export function id(value: unknown, name = "id"): asserts value is string {
  assert(typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9_.-]{0,95}$/.test(value), `${name}: invalid identifier`);
}
export function hash(value: unknown): asserts value is import("./types.ts").Digest {
  assert(typeof value === "string" && /^sha256:[0-9a-f]{64}$/.test(value), "Invalid SHA-256 digest");
}
/** Integer-only, recursively sorted JSON. Deliberately separate from legacy publication hashing. */
export function canonical(value: unknown): string {
  if (value === null || typeof value === "boolean" || typeof value === "string") return JSON.stringify(value);
  if (typeof value === "number") { assert(Number.isSafeInteger(value), "Canonical quantities must be safe integers"); return JSON.stringify(value); }
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  assert(value !== undefined && typeof value === "object" && Object.getPrototypeOf(value) === Object.prototype, "Expected plain canonical JSON value");
  return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical((value as Record<string, unknown>)[key])}`).join(",")}}`;
}
export async function digest(bytes: Uint8Array): Promise<import("./types.ts").Digest> {
  const result = await globalThis.crypto.subtle.digest("SHA-256", Uint8Array.from(bytes).buffer);
  return `sha256:${Array.from(new Uint8Array(result), byte => byte.toString(16).padStart(2, "0")).join("")}`;
}
export function freeze<T>(value: T): T {
  if (value && typeof value === "object") { Object.values(value).forEach(freeze); Object.freeze(value); }
  return value;
}
