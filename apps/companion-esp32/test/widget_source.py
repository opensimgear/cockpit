"""Differential source admission, bounded ownership, ASan/UBSan and coverage."""
import gzip
import json
import os
from pathlib import Path
import subprocess

APP = Path(__file__).resolve().parents[1]
OUT = APP.parents[1] / '.research/esp32-widget-source-native'
OUT.mkdir(parents=True, exist_ok=True)
for counter in OUT.glob('widget-source-test-*.gcda'):
    counter.unlink()
flags = ['-std=c11', '-O1', '-g', '--coverage', '-fsanitize=address,undefined', '-fno-omit-frame-pointer', '-Wall', '-Wextra', '-Werror', '-I' + str(APP / 'core'), '-I' + str(APP / 'render'), '-I' + str(APP / 'third_party/cjson')]
subprocess.run([os.environ.get('CC', 'gcc'), *flags, str(APP / 'render/widget_source.c'), str(APP / 'third_party/cjson/cJSON.c'), str(APP / 'test/widget_source_test.c'), '-lm', '-o', str(OUT / 'widget-source-test')], check=True)
result = subprocess.run([str(OUT / 'widget-source-test'), str(APP / 'test/widget-source.json')], capture_output=True, text=True, timeout=60, env={**os.environ, 'UBSAN_OPTIONS': 'halt_on_error=1:print_stacktrace=1'})
(OUT / 'widget-source-test.log').write_text(result.stdout + result.stderr)
print(result.stdout + result.stderr, end='')
result.check_returncode()
subprocess.run(['gcov', '-j', '-b', '-c', 'widget-source-test-widget_source.gcno'], check=True, cwd=OUT, stdout=subprocess.DEVNULL)
coverage = json.loads(gzip.decompress((OUT / 'widget-source-test-widget_source.gcov.json.gz').read_bytes()))
module = next(f for f in coverage['files'] if f['file'].endswith('/widget_source.c'))
lines, functions = module['lines'], module['functions']
branches = [b for line in lines for b in line.get('branches', [])]
metrics = {'lines': 100 * sum(line['count'] > 0 for line in lines) / len(lines), 'branches': 100 * sum(branch['count'] > 0 for branch in branches) / len(branches), 'functions': 100 * sum(f['execution_count'] > 0 for f in functions) / len(functions)}
(OUT / 'coverage.json').write_text(json.dumps(metrics, indent=2) + '\n')
print('Widget source admission coverage:', metrics)
if any(value < 71 for value in metrics.values()):
    raise RuntimeError('Widget source admission coverage below 71%')
