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
cache = sha256((APP / 'render/lv_conf.h').read_bytes() + (APP / 'third_party/lvgl-font-cache.patch.json').read_bytes() + json.dumps(flags).encode()).hexdigest()
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
font_object = OUT / 'portable-fonts.o'
subprocess.run(['ld', '-r', '-b', 'binary', 'osg-sans.ttf', 'osg-mono.ttf', 'OFL-Rajdhani.txt', 'OFL-IBMPlexMono.txt', '-o', str(font_object)], check=True, cwd=ROOT / 'packages/companion-ui/assets/fonts')
subprocess.run(['objcopy', '--rename-section', '.data=.rodata,alloc,load,readonly,data,contents', '--add-section', '.note.GNU-stack=/dev/null', '--set-section-flags', '.note.GNU-stack=noload,readonly', str(font_object)], check=True)
for counter in OUT.glob('lvgl-test-*.gcda'):
    counter.unlink()
subprocess.run([os.environ.get('CC', 'gcc'), *flags, '--coverage', '-Wall', '-Wextra', '-Werror', str(APP / 'render/lvgl_backend.c'), str(APP / 'render/widget_fonts.c'), str(APP / 'test/lvgl_test.c'), str(APP / 'third_party/cjson/cJSON.c'), str(font_object), str(library), '-lm', '-o', str(OUT / 'lvgl-test')], check=True)
test_environment = {**os.environ, 'UBSAN_OPTIONS': 'halt_on_error=1:print_stacktrace=1'}
subprocess.run([str(OUT / 'lvgl-test'), str(APP / 'test/widget-lvgl.json'), str(OUT)], check=True, timeout=60, env=test_environment)
report = {}
for name in ['lvgl_backend', 'widget_fonts']:
    subprocess.run(['gcov', '-j', '-b', '-c', f'lvgl-test-{name}.gcno'], cwd=OUT, check=True, capture_output=True)
    coverage = json.loads(gzip.decompress((OUT / f'lvgl-test-{name}.gcov.json.gz').read_bytes()))
    module = next(f for f in coverage['files'] if f['file'].endswith('/' + name + '.c'))
    lines, functions = module['lines'], module['functions']
    branches = [b for line in lines for b in line.get('branches', [])]
    report[name] = {'lines': 100 * sum(line['count'] > 0 for line in lines) / len(lines), 'branches': 100 * sum(branch['count'] > 0 for branch in branches) / len(branches), 'functions': 100 * sum(f['execution_count'] > 0 for f in functions) / len(functions)}
(OUT / 'coverage.json').write_text(json.dumps(report, indent=2) + '\n')
if any(value < 71 for metrics in report.values() for value in metrics.values()):
    raise RuntimeError('LVGL module coverage below 71%: ' + str(report))
print('LVGL module coverage:', report)


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


for ppm in OUT.glob('*.ppm'):
    magic, dimensions, maximum, rgb = ppm.read_bytes().split(b'\n', 3)
    assert magic == b'P6' and maximum == b'255'
    width, height = map(int, dimensions.split())
    rows = b''.join(b'\0' + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))
    ppm.with_suffix('.png').write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))
print('Rendered states:', OUT)
