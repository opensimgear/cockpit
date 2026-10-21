"""Reproduce portable glyph advance/baseline metadata from the pinned font files.

This reads standard TrueType head/hhea/hmtx/cmap tables; it does not modify fonts.
The browser and LVGL use the same em sizes, unkerned integer glyph advances and
baseline rule. No font preparation dependency is needed at runtime or build time.
"""
from hashlib import sha256
import json
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / 'packages/companion-ui/assets/fonts'
SOURCES = {
    'sans': ('osg-sans.ttf', 'Rajdhani SemiBold', '94bbd25a18ca665999feb05a537de9fd2b860dcfb78bbe9ca00270825bf235da'),
    'mono': ('osg-mono.ttf', 'IBM Plex Mono Regular', '6a3412f058c7d8dfd9170c41e85ade48e5156ecb89356110ca57a0a27734af46'),
}


def metadata(filename, title, digest):
    data = (ASSETS / filename).read_bytes()
    assert sha256(data).hexdigest() == digest, 'Portable font digest mismatch'
    u16 = lambda at: struct.unpack_from('>H', data, at)[0]
    i16 = lambda at: struct.unpack_from('>h', data, at)[0]
    u32 = lambda at: struct.unpack_from('>I', data, at)[0]
    tables = {data[at:at + 4].decode('ascii'): u32(at + 8) for at in range(12, 12 + u16(4) * 16, 16)}
    hhea, cmap, head, hmtx = [tables[key] for key in ('hhea', 'cmap', 'head', 'hmtx')]
    count = u16(hhea + 34)
    widths = [u16(hmtx + i * 4) for i in range(count)]
    mapping = {}
    for at in range(cmap + 4, cmap + 4 + u16(cmap + 2) * 8, 8):
        platform, encoding, offset = u16(at), u16(at + 2), cmap + u32(at + 4)
        if platform != 0 and not (platform == 3 and encoding in (1, 10)):
            continue
        if u16(offset) == 12:
            for group in range(offset + 16, offset + 16 + u32(offset + 12) * 12, 12):
                start, end, glyph = u32(group), u32(group + 4), u32(group + 8)
                assert end <= 0x10ffff and end - start < 65536
                for code in range(start, end + 1):
                    mapping[code] = glyph + code - start
        elif u16(offset) == 4:
            segments = u16(offset + 6) // 2
            ends = offset + 14
            starts, deltas, ranges = ends + segments * 2 + 2, ends + segments * 4 + 2, ends + segments * 6 + 2
            for i in range(segments):
                start, end, delta, distance = u16(starts + i * 2), u16(ends + i * 2), i16(deltas + i * 2), u16(ranges + i * 2)
                for code in range(start, min(end, 0xfffe) + 1):
                    glyph = u16(ranges + i * 2 + distance + (code - start) * 2) if distance else code
                    if glyph or not distance:
                        mapping[code] = (glyph + delta) & 0xffff
    advances = [[code, widths[min(glyph, count - 1)]] for code, glyph in sorted(mapping.items()) if code >= 32 and glyph]
    return {'name': title, 'sha256': digest, 'unitsPerEm': u16(head + 18), 'ascent': i16(hhea + 4), 'descent': i16(hhea + 6), 'lineGap': i16(hhea + 8), 'missingAdvance': widths[0], 'advances': advances}


text = json.dumps({'formatVersion': 1, 'fonts': {key: metadata(*value) for key, value in SOURCES.items()}}, separators=(',', ':'), ensure_ascii=False) + '\n'
path = ASSETS / 'metrics.json'
if '--check' in sys.argv:
    assert path.read_text().replace('\r\n', '\n') == text, 'Regenerate portable font metrics'
else:
    path.write_text(text, encoding='utf-8', newline='\n')
print('Portable font digests and metrics verified')
