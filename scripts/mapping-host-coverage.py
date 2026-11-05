#!/usr/bin/env python3
"""Measure each mapping host module and require at least 71% production line coverage."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from native_coverage_sources import measure_module


def run(args, root, env=None):
    try:
        return subprocess.check_output([str(arg) for arg in args], cwd=root, env=env, text=True, encoding="utf-8")
    except subprocess.CalledProcessError as error:
        print(error.output or '', file=sys.stderr, end='')
        raise


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=root / '.research/mapping-host-coverage')
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profiles = Path(tempfile.mkdtemp(prefix='profiles-', dir=output))
    target = Path(os.environ.get('CARGO_TARGET_DIR', str(output / 'target')))
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), RUSTFLAGS='-C instrument-coverage',
               LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw'))
    # Only host coverage is measured here. Instrumenting the separate real helper
    # adds overhead inside its strict evaluation/response deadlines; its own
    # coverage is measured by logic-coverage.py. Keep that process representative
    # of the normal debug helper without relaxing any runtime budget.
    helper_target = output / 'logic-helper-target'
    helper_env = dict(os.environ, CARGO_TARGET_DIR=str(helper_target))
    for key in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'LLVM_PROFILE_FILE'):
        helper_env.pop(key, None)
    run(['cargo', 'build', '--locked', '--bin', 'osg-logic', '--manifest-path',
         root / 'crates/logic/Cargo.toml'], root, helper_env)
    env['OSG_TEST_LOGIC_WORKER'] = str(helper_target / 'debug' / ('osg-logic.exe' if os.name == 'nt' else 'osg-logic'))
    artifacts = run(['cargo', 'test', '--locked', '--lib', '--no-run', '--message-format=json',
                     '--manifest-path', root / 'crates/mapping-host/Cargo.toml'], root, env)
    binaries = [entry['executable'] for line in artifacts.splitlines()
                if (entry := json.loads(line)).get('reason') == 'compiler-artifact'
                and entry.get('executable') and entry.get('profile', {}).get('test')]
    if len(binaries) != 1:
        raise RuntimeError('Expected one mapping host library test executable.')
    # Host code runs in this process, including its capture/feeder fixture threads.
    # Real script helpers are deliberately killed by lifecycle tests and can leave
    # incomplete LLVM profiles. Their coverage belongs to logic-coverage.py.
    # Select the successful host PID, never silently ignore corrupt host evidence.
    test_env = dict(env, LLVM_PROFILE_FILE=str(profiles / 'host-%m-%p.profraw'))
    tests = subprocess.Popen([binaries[0], "--test-threads=1"], cwd=root, env=test_env, text=True, encoding='utf-8',
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    test_output, _ = tests.communicate()
    (output / 'tests.log').write_text(test_output, encoding='utf-8')
    if tests.returncode:
        print(test_output, flush=True)
        raise subprocess.CalledProcessError(tests.returncode, tests.args)
    host_profiles = list(profiles.glob(f'host-*-{tests.pid}.profraw'))
    if not host_profiles:
        raise RuntimeError('The successful mapping-host process produced no coverage profile.')
    sysroot = Path(run(['rustc', '--print', 'sysroot'], root).strip())
    host = re.search(r'^host: (.+)$', run(['rustc', '-vV'], root), re.M).group(1)
    suffix = '.exe' if os.name == 'nt' else ''
    llvm = sysroot / 'lib/rustlib' / host / 'bin'
    profile = output / 'coverage.profdata'
    run([llvm / f'llvm-profdata{suffix}', 'merge', '-sparse', *host_profiles, '-o', profile], root)
    sources = [root / f'crates/mapping-host/src/{module}.rs' for module in [
        'learn', 'router', 'service', 'settings', 'panels', 'game', 'companions',
        'companion_native', 'companion_runs', 'typed_sources', 'desktop',
        'desktop/dispatch', 'desktop/deadlines', 'desktop/lifecycle',
        'desktop/output', 'desktop/output/worker', 'desktop/output/execute']]
    summary = {}
    for source in sources:
        result, annotated = measure_module(source, lambda path: run([
            llvm / f'llvm-cov{suffix}', 'show', binaries[0], f'-instr-profile={profile}',
            '-show-line-counts-or-regions', path], root), root)
        module = source.relative_to(root / 'crates/mapping-host/src').with_suffix('').as_posix()
        (output / f'{module.replace("/", "-")}-lines.txt').write_text(annotated, encoding='utf-8')
        summary[source.relative_to(root / 'crates/mapping-host/src').as_posix()] = result
        print(f'{module}: {result["covered"]}/{result["lines"]} production lines ({result["percent"]:.2f}%)', flush=True)
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    if any(item['percent'] < 71 for item in summary.values()):
        raise SystemExit('Each mapping host module must meet 71% production line coverage.')


if __name__ == '__main__':
    main()
