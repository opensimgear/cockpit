"""Portable binary-object structure/budget checks. Semantic/source admission is separate."""
from pathlib import Path
import gzip
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / 'apps/companion-esp32'
OUT = ROOT / '.research/esp32-render-package-native'
OUT.mkdir(parents=True, exist_ok=True)
for counter in OUT.glob('package-test-*.gcda'):
    counter.unlink()
subprocess.run([os.environ.get('CC', 'gcc'), '-std=c11', '-Wall', '-Wextra', '-Werror', '-pedantic', '-g', '-fsanitize=address,undefined', '--coverage', str(APP / 'render/package.c'), str(APP / 'test/package_test.c'), '-o', str(OUT / 'package-test')], check=True)
result = subprocess.run([str(OUT / 'package-test')], capture_output=True, text=True, timeout=60)
(OUT / 'package-test.log').write_text(result.stdout + result.stderr)
print(result.stdout + result.stderr, end='')
result.check_returncode()
subprocess.run(['gcov', '-j', '-b', '-c', 'package-test-package.gcno'], check=True, cwd=OUT, stdout=subprocess.DEVNULL)
data = json.loads(gzip.decompress((OUT / 'package-test-package.gcov.json.gz').read_bytes()))
module = next(file for file in data['files'] if file['file'].endswith('/render/package.c'))
lines, functions = module['lines'], module['functions']
branches = [branch for line in lines for branch in line.get('branches', [])]
metrics = {'lines': 100 * sum(line['count'] > 0 for line in lines) / len(lines), 'branches': 100 * sum(branch['count'] > 0 for branch in branches) / len(branches), 'functions': 100 * sum(function['execution_count'] > 0 for function in functions) / len(functions)}
(OUT / 'coverage.json').write_text(json.dumps(metrics, indent=2) + '\n')
print(json.dumps(metrics, indent=2))
if any(value < 71 for value in metrics.values()):
    raise SystemExit('Render object inspector coverage below 71%')
