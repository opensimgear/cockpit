#!/usr/bin/env python3
"""Measure each unified profile module and require at least 71% production line coverage."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from native_coverage_sources import measure_module


def run(args, root, env=None, log=None):
    result = subprocess.run([str(arg) for arg in args], cwd=root, env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, encoding="utf-8")
    if log is not None:
        log.write_text(result.stdout + result.stderr, encoding='utf-8')
    if result.returncode:
        print(result.stdout + result.stderr, file=sys.stderr, flush=True)
        result.check_returncode()
    elif result.stderr:
        print(result.stderr, file=sys.stderr, end='', flush=True)
    return result.stdout


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=root / '.research/profiles-coverage')
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profiles = Path(tempfile.mkdtemp(prefix='profiles-', dir=output))
    env = dict(os.environ, CARGO_TARGET_DIR=str(output / 'target'), RUSTFLAGS='-C instrument-coverage',
               LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw'))
    artifacts = run(['cargo', 'test', '--locked', '--lib', '--no-run', '--message-format=json',
                     '--manifest-path', root / 'crates/profiles/Cargo.toml'], root, env)
    binaries = [entry['executable'] for line in artifacts.splitlines()
                if (entry := json.loads(line)).get('reason') == 'compiler-artifact'
                and entry.get('executable') and entry.get('profile', {}).get('test')]
    if len(binaries) != 1:
        raise RuntimeError('Expected one profiles library test executable.')
    run([binaries[0]], root, env, output / 'tests.log')
    sysroot = Path(run(['rustc', '--print', 'sysroot'], root).strip())
    host = re.search(r'^host: (.+)$', run(['rustc', '-vV'], root), re.M).group(1)
    suffix = '.exe' if os.name == 'nt' else ''
    llvm = sysroot / 'lib/rustlib' / host / 'bin'
    profile = output / 'coverage.profdata'
    run([llvm / f'llvm-profdata{suffix}', 'merge', '-sparse', *profiles.glob('*.profraw'), '-o', profile], root)
    sources = [root / f'crates/profiles/src/{module}.rs' for module in ['desktop_blocks', 'debug_controller', 'lib', 'activation', 'model', 'validation', 'session', 'store', 'service', 'service/read_model', 'virtual_slots', 'resources', 'resources/games', 'resources_companion', 'resources_ports', 'resources_store', 'compiler', 'compiler_models', 'compiler_capture', 'compiler_conflicts', 'compiler_hid_policy']]
    summary = {}
    for source in sources:
        result, annotated = measure_module(source, lambda path: run([
            llvm / f'llvm-cov{suffix}', 'show', binaries[0], f'-instr-profile={profile}',
            '-show-line-counts-or-regions', path], root), root)
        name = str(source.relative_to(root / 'crates/profiles/src'))
        (output / (name.replace('\\', '-').replace('/', '-') + '-lines.txt')).write_text(annotated, encoding='utf-8')
        summary[name] = result
        print(f"{name}: {result['covered']}/{result['lines']} production lines ({result['percent']:.2f}%)", flush=True)
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    if any(item['percent'] < 71 for item in summary.values()):
        raise SystemExit('Each profiles module must meet 71% production line coverage.')


if __name__ == '__main__':
    main()
