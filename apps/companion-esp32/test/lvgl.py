"""Build real pinned LVGL and the board adapter with ASan/UBSan, then render source fixtures."""
from concurrent.futures import ThreadPoolExecutor
from hashlib import sha256
import gzip
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import zlib

APP = Path(__file__).resolve().parents[1]
ROOT = APP.parents[1]
sys.path.insert(0, str(APP / 'tools'))
from lvgl_source import source

LVGL = source()
OUT = ROOT / '.research/lvgl-native'
OUT.mkdir(parents=True, exist_ok=True)
flags = ['-std=c11', '-O1', '-g', '-fsanitize=address,undefined', '-fno-omit-frame-pointer', '-DLV_CONF_INCLUDE_SIMPLE', '-DLV_KCONFIG_IGNORE', '-I' + str(APP / 'render'), '-I' + str(LVGL), '-I' + str(APP / 'boards'), '-I' + str(APP / 'core'), '-I' + str(APP / 'third_party/cjson')]
cache = sha256((APP / 'render/lv_conf.h').read_bytes() + json.dumps(flags).encode()).hexdigest()
objects = OUT / cache
objects.mkdir(exist_ok=True)


def compile_lvgl(path):
    obj = objects / (str(path.relative_to(LVGL)).replace('/', '_') + '.o')
    if not obj.exists():
        result = subprocess.run([os.environ.get('CC', 'gcc'), *flags, '-c', str(path), '-o', str(obj)], capture_output=True, text=True)
        if result.returncode:
            raise RuntimeError(result.stderr)
    return obj


sources = sorted((LVGL / 'src').rglob('*.c'))
print('Building', len(sources), 'locked LVGL translation units', flush=True)
with ThreadPoolExecutor(max_workers=min(8, os.cpu_count() or 1)) as pool:
    compiled = list(pool.map(compile_lvgl, sources))
library = objects / 'liblvgl.a'
subprocess.run(['ar', 'rcs', str(library), *map(str, compiled)], check=True)
for counter in OUT.glob('lvgl-test-*.gcda'):
    counter.unlink()
subprocess.run([os.environ.get('CC', 'gcc'), *flags, '--coverage', '-Wall', '-Wextra', '-Werror', str(APP / 'render/lvgl_backend.c'), str(APP / 'test/lvgl_test.c'), str(APP / 'third_party/cjson/cJSON.c'), str(library), '-lm', '-o', str(OUT / 'lvgl-test')], check=True)
subprocess.run([str(OUT / 'lvgl-test'), str(APP / 'test/widget-lvgl.json'), str(OUT)], check=True, timeout=60)
subprocess.run(['gcov', '-j', '-b', '-c', 'lvgl-test-lvgl_backend.gcno'], cwd=OUT, check=True, capture_output=True)
coverage = json.loads(gzip.decompress((OUT / 'lvgl-test-lvgl_backend.gcov.json.gz').read_bytes()))
module = next(f for f in coverage['files'] if f['file'].endswith('/lvgl_backend.c'))
lines, functions = module['lines'], module['functions']
branches = [b for line in lines for b in line.get('branches', [])]
metrics = {'lines': 100 * sum(line['count'] > 0 for line in lines) / len(lines), 'branches': 100 * sum(branch['count'] > 0 for branch in branches) / len(branches), 'functions': 100 * sum(f['execution_count'] > 0 for f in functions) / len(functions)}
(OUT / 'coverage.json').write_text(json.dumps(metrics, indent=2) + '\n')
if any(value < 71 for value in metrics.values()):
    raise RuntimeError('LVGL adapter coverage below 71%: ' + str(metrics))
print('Adapter coverage:', metrics)


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


for ppm in OUT.glob('*.ppm'):
    magic, dimensions, maximum, rgb = ppm.read_bytes().split(b'\n', 3)
    assert magic == b'P6' and maximum == b'255'
    width, height = map(int, dimensions.split())
    rows = b''.join(b'\0' + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))
    ppm.with_suffix('.png').write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))
print('Rendered states:', OUT)
