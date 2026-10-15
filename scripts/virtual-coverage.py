#!/usr/bin/env python3
"""Measure each virtual controller module and require at least 71% production line coverage."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


def run(args, root, env=None):
    try:
        return subprocess.check_output([str(arg) for arg in args], cwd=root, env=env, text=True, encoding="utf-8")
    except subprocess.CalledProcessError as error:
        # Keep the test failure visible in CI instead of discarding captured stdout.
        print(error.output or '', file=sys.stderr, end='')
        raise


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=root / '.research/virtual-coverage')
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    profiles = Path(tempfile.mkdtemp(prefix='profiles-', dir=output))
    env = dict(os.environ, OSG_TEST_PYTHON=sys.executable,
               CARGO_TARGET_DIR=str(output / 'target'), RUSTFLAGS='-C instrument-coverage',
               LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw'))
    artifacts = run(['cargo', 'test', '--locked', '--lib', '--no-run', '--message-format=json',
                     '--manifest-path', root / 'crates/virtual-controller/Cargo.toml'], root, env)
    binaries = [entry['executable'] for line in artifacts.splitlines()
                if (entry := json.loads(line)).get('reason') == 'compiler-artifact'
                and entry.get('executable') and entry.get('profile', {}).get('test')]
    if len(binaries) != 1:
        raise RuntimeError('Expected one virtual controller library test executable.')
    (output / 'tests.log').write_text(run([binaries[0]], root, env), encoding='utf-8')
    sysroot = Path(run(['rustc', '--print', 'sysroot'], root).strip())
    host = re.search(r'^host: (.+)$', run(['rustc', '-vV'], root), re.M).group(1)
    suffix = '.exe' if os.name == 'nt' else ''
    llvm = sysroot / 'lib/rustlib' / host / 'bin'
    profile = output / 'coverage.profdata'
    run([llvm / f'llvm-profdata{suffix}', 'merge', '-sparse', *profiles.glob('*.profraw'), '-o', profile], root)
    sources = [root / f'crates/virtual-controller/src/{module}.rs' for module in (['hid', 'apple_helper', 'uinput', 'descriptor', 'vjoy', 'feeder', 'worker', 'supervisor', 'provision', 'inspection', 'session', 'entry', 'output_journal'] + (['windows', 'registry', 'windows_provision', 'journal_security', 'windows_refresh', 'windows_process', 'windows_coordination', 'windows_launcher', 'windows_provision_entry', 'windows_jobs'] if os.name == 'nt' else ['linux_uinput'] if sys.platform == 'linux' else ['macos_hid'] if sys.platform == 'darwin' else []))]
    summary = {}
    for source in sources:
        annotated = run([llvm / f'llvm-cov{suffix}', 'show', binaries[0], f'-instr-profile={profile}',
                         '-show-line-counts-or-regions', source], root)
        (output / f'{source.stem}-lines.txt').write_text(annotated, encoding='utf-8')
        lines = source.read_text(encoding='utf-8').splitlines()
        end = next((i + 1 for i, line in enumerate(lines) if line.strip() == '#[cfg(test)]'), len(lines) + 1)
        counts = {int(match[1]): int(match[2]) for line in annotated.splitlines()
                  if (match := re.match(r'^\s*(\d+)\|\s*(\d+)\|', line)) and int(match[1]) < end}
        if not counts:
            raise RuntimeError(f'No instrumented production lines for {source.name}.')
        covered = sum(count > 0 for count in counts.values())
        percent = 100 * covered / len(counts)
        summary[source.name] = dict(covered=covered, lines=len(counts), percent=round(percent, 2))
        print(f'{source.name}: {covered}/{len(counts)} production lines ({percent:.2f}%)', flush=True)
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    if any(item['percent'] < 71 for item in summary.values()):
        raise SystemExit('Each virtual controller module must meet 71% production line coverage.')


if __name__ == '__main__':
    main()
