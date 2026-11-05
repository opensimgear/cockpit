#!/usr/bin/env python3
"""Measure each portable mapping module and require at least 71% production line coverage."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


def run(args, root, env=None):
    return subprocess.check_output([str(arg) for arg in args], cwd=root, env=env, text=True, encoding="utf-8")


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=root / '.research/mapping-coverage')
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profiles = Path(tempfile.mkdtemp(prefix='profiles-', dir=output))
    env = dict(os.environ, CARGO_TARGET_DIR=os.environ.get('CARGO_TARGET_DIR', str(output / 'target')), RUSTFLAGS='-C instrument-coverage',
               LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw'))
    artifacts = run(['cargo', 'test', '--locked', '--lib', '--no-run', '--message-format=json',
                     '--manifest-path', root / 'crates/mapping/Cargo.toml'], root, env)
    binaries = [entry['executable'] for line in artifacts.splitlines()
                if (entry := json.loads(line)).get('reason') == 'compiler-artifact'
                and entry.get('executable') and entry.get('profile', {}).get('test')]
    if len(binaries) != 1:
        raise RuntimeError('Expected one mapping library test executable.')
    tests = subprocess.run([binaries[0]], cwd=root, env=env, text=True, encoding='utf-8',
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (output / 'tests.log').write_text(tests.stdout, encoding='utf-8')
    if tests.returncode:
        print(tests.stdout, flush=True)
        tests.check_returncode()
    sysroot = Path(run(['rustc', '--print', 'sysroot'], root).strip())
    host = re.search(r'^host: (.+)$', run(['rustc', '-vV'], root), re.M).group(1)
    suffix = '.exe' if os.name == 'nt' else ''
    llvm = sysroot / 'lib/rustlib' / host / 'bin'
    profile = output / 'coverage.profdata'
    run([llvm / f'llvm-profdata{suffix}', 'merge', '-sparse', *profiles.glob('*.profraw'), '-o', profile], root)
    sources = [root / f'crates/mapping/src/{module}.rs' for module in ['script', 'script_engine', 'rules', 'rules/seed', 'profile', 'engine', 'runtime', 'store', 'trace', 'documents', 'signal', 'program', 'program_validation', 'typed_engine', 'typed_engine/deadlines', 'typed_engine/evaluate', 'desktop']]
    native_modules = ['mod', 'ledger', 'validate', 'admission', 'budget', 'fences',
                      'proof', 'recovery', 'retained_bytes', 'revocations', 'graph', 'liveness', 'desktop',
                      'scripts', 'output_work', 'publish', 'direct_apply', 'direct_batch', 'presentation']
    sources.extend(root / f'crates/mapping/src/typed_engine/native/{name}.rs'
                   for name in native_modules)
    summary = {}
    for source in sources:
        name = source.relative_to(root / 'crates/mapping/src').as_posix()
        annotated = run([llvm / f'llvm-cov{suffix}', 'show', binaries[0], f'-instr-profile={profile}',
                         '-show-line-counts-or-regions', source], root)
        (output / (name.replace('/', '-') + '-lines.txt')).write_text(annotated, encoding='utf-8')
        lines = source.read_text(encoding='utf-8').splitlines()
        end = next((i + 1 for i, line in enumerate(lines) if line.strip() == '#[cfg(test)]'), len(lines) + 1)
        counts = {int(match[1]): float(match[2].rstrip("kMGT")) > 0 for line in annotated.splitlines()
                  if (match := re.match(r'^\s*(\d+)\|\s*([0-9]+(?:\.[0-9]+)?[kMGT]?)\|', line)) and int(match[1]) < end}
        if not counts:
            raise RuntimeError(f'No instrumented production lines for {source.name}.')
        covered = sum(counts.values())
        percent = 100 * covered / len(counts)
        summary[name] = dict(covered=covered, lines=len(counts), percent=round(percent, 2))
        print(f'{name}: {covered}/{len(counts)} production lines ({percent:.2f}%)', flush=True)
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    if any(item['percent'] < 71 for item in summary.values()):
        raise SystemExit('Each mapping module must meet 71% production line coverage.')


if __name__ == '__main__':
    main()
