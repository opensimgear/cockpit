"""Whole portable panel admission, ownership, rollback and shared-model oracle."""
import gzip
import json
import os
from pathlib import Path
import subprocess

APP = Path(__file__).resolve().parents[1]
OUT = APP.parents[1] / '.research/esp32-panel-source-native'
OUT.mkdir(parents=True, exist_ok=True)
for counter in OUT.glob('panel-source-test-*.gcda'):
    counter.unlink()
flags = ['-std=c11', '-O1', '-g', '--coverage', '-fsanitize=address,undefined', '-fno-omit-frame-pointer', '-Wall', '-Wextra', '-Werror']
flags += ['-DCJSON_NESTING_LIMIT=24','-DCJSON_CIRCULAR_LIMIT=2048']
flags += ['-I' + str(APP / p) for p in ['core', 'render', 'network', 'third_party/cjson']]
sources = ['core/companion.c', 'core/widget.c', 'render/widget_state.c', 'render/widget_source.c', 'network/json.c', 'network/panel_source.c', 'third_party/cjson/cJSON.c', 'test/panel_source_test.c']
subprocess.run([os.environ.get('CC','gcc'), *flags, *[str(APP / p) for p in sources], '-lm', '-lcrypto', '-o', str(OUT / 'panel-source-test')], check=True)
result = subprocess.run([str(OUT / 'panel-source-test'), str(APP / 'test/panel-source.json')], capture_output=True, text=True, timeout=120, env={**os.environ,'UBSAN_OPTIONS':'halt_on_error=1:print_stacktrace=1'})
(OUT / 'panel-source-test.log').write_text(result.stdout + result.stderr)
print(result.stdout + result.stderr, end='')
result.check_returncode()
subprocess.run(['gcov','-j','-b','-c','panel-source-test-panel_source.gcno'], check=True, cwd=OUT, stdout=subprocess.DEVNULL)
data = json.loads(gzip.decompress((OUT / 'panel-source-test-panel_source.gcov.json.gz').read_bytes()))
module = next(f for f in data['files'] if f['file'].endswith('/panel_source.c'))
lines, functions = module['lines'], module['functions']
branches = [b for line in lines for b in line.get('branches',[])]
metrics = {'lines':100*sum(l['count']>0 for l in lines)/len(lines),'branches':100*sum(b['count']>0 for b in branches)/len(branches),'functions':100*sum(f['execution_count']>0 for f in functions)/len(functions)}
(OUT / 'coverage.json').write_text(json.dumps(metrics,indent=2)+'\n')
print('Complete panel source coverage:',metrics)
if min(metrics.values()) < 71:
    raise RuntimeError('Complete panel source coverage below 71%')
