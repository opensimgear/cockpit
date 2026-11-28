# Authored HID contract corpus

Run from the cockpit repository root:

```sh
node tests/hid-contract/conformance.mjs
node --test tests/hid-contract/conformance.test.mjs
node --experimental-test-coverage --test-coverage-include=tests/hid-contract/conformance.mjs --test-coverage-lines=90 --test-coverage-branches=80 --test-coverage-functions=90 --test tests/hid-contract/conformance.test.mjs
```

The corpus has seven authored descriptors, eleven report declarations, eighteen golden
reports, and one truncated descriptor case. Every declared input, output, and
feature layout has a sample. No device is opened, read, configured, or written.
Output and feature bytes are inert reference data. These authored examples are
not physical-device captures or operating-system qualification evidence.

| Fixture | Authored coverage |
| --- | --- |
| boot-keyboard | Unnumbered modifier bits, six-key array, reserved byte, LED output |
| relative-mouse | Signed relative X/Y/wheel, buttons, identical consecutive movement events, nonzero constant padding |
| packed-joystick | Report-ID prefix, cross-byte signed 12-bit axes, null hat, button usages |
| hat-domains | Four-way and continuous-angle hats, explicit degree unit/exponent, asymmetric unsigned axis with unknown neutral |
| vendor-wide | Raw unsigned/signed 64-bit values, exact values above 32 and 53 bits, feature report |
| vendor-multikind | Same ID with independently addressed input/output/feature layouts and lengths |
| metadata-preservation | Push/Pop, delimiter branch, strings/designators, nested collections, opaque long/reserved items |
| malformed/truncated-usage | Literal truncated item prefix, offset, declared and available payload bytes |

## Representation

`descriptorHex` and `wireHex` are nonempty lowercase hex bytes in transport order.
`reportId: null` means an unnumbered report and contributes no framing byte.
IDs 1–255 require one matching leading byte. Zero is not a numbered-report ID.
Each `(kind, reportId)` pair has its own payload size and field layout. Field
offsets count LSB-first bits in the payload, excluding report-ID framing.

All logical bounds and expected scalar values are canonical decimal strings.
JSON numbers, leading zeros, `-0`, exponent notation, and rounded wide values
are rejected. Arrays retain individual exact selectors. Null hat encodings and
raw values outside the logical range have separate explicit expectations.
The wide fixture intentionally exceeds its descriptor's 32-bit logical bounds:
its exact raw 64-bit values must survive with an out-of-range classification,
not be clamped or converted through JavaScript `Number`.

Descriptor bytes, declared layout offsets, and expected values are authored
goldens. The checker reads the actual golden payload bits, sign-extends using
`BigInt`, compares every element to its decimal expectation, independently packs
those expectations, and compares every payload byte. An altered golden bit or
rounded expected value therefore fails. Padding must be declared and its raw
bits are preserved, including the mouse's nonzero constant-padding sample;
constant bits are not classified against inherited logical bounds. Metadata
checks compare exact literal bytes at their
golden descriptor offsets. Truncation checks compare the actual short-item size
bits with available bytes. Repeated relative reports remain distinct ordered
sample records even when their bytes are identical.

## Scope and limits

This is H0 fixture conformance, not an H1 HID descriptor decoder. It does not
infer fields, collection scope, usages, flags, or global/local state from HID
items. The metadata fixture records explicit H1 interpretation/preservation
expectations; H0 proves their literal offsets and bytes. Future H1 tests should
compare decoded descriptors with these declared goldens, then reuse the report
vectors. Physical capture evidence and platform coverage remain separate.

Fixture bounds are deliberately small: 4,096 descriptor bytes, 4,096 payload
bits, 128 bits per scalar, 256 fields per report, 256 elements per field, 32
report declarations, and 256 samples per fixture. Every payload bit must have
one declared field; overlap and holes fail. These are corpus admission bounds,
not claims about production HID support or a complete HID grammar.

Validation at creation: 13 tests passed; checker coverage was 100% lines
and functions. Negative checks cover mutated golden bits,
wide-value rounding, report-ID framing, report-kind collisions, metadata drift,
null/overflow classification, fixture bounds, overlaps/holes, missing samples,
and malformed integer/byte representations.
