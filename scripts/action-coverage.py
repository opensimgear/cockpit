#!/usr/bin/env python3
"""Run native tests and gate production device-action line coverage above 70%."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


def command(args, cwd, env=None, log_path=None):
    result = subprocess.run(args, cwd=cwd, env=env, text=True, encoding='utf-8',
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if log_path is not None:
        log_path.write_text(result.stdout, encoding='utf-8')
    if result.returncode:
        print(result.stdout, file=sys.stderr, end='')
        result.check_returncode()
    return result.stdout


def cargo_test_binaries(artifacts):
    return [entry['executable'] for line in artifacts.splitlines() if line.startswith('{')
            if (entry := json.loads(line)).get('reason') == 'compiler-artifact'
            and entry.get('executable') and entry.get('profile', {}).get('test')]


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=root / '.research/action-coverage')
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for name in ['native-tests.log', 'action-summary.json', 'lines.txt']:
        (output / name).unlink(missing_ok=True)
    build_profiles = output / 'build-profiles'
    build_profiles.mkdir(exist_ok=True)
    environment = dict(os.environ, CARGO_TARGET_DIR=os.environ.get('CARGO_TARGET_DIR', str(output / 'target')),
                       RUSTFLAGS='-C instrument-coverage',
                       LLVM_PROFILE_FILE=str(build_profiles / '%m-%p.profraw'))
    artifacts = command(['cargo', 'test', '--locked', '--lib', '--no-run', '--message-format=json',
                         '--manifest-path', str(root / 'src-tauri/Cargo.toml')], root, environment)
    binaries = cargo_test_binaries(artifacts)
    if len(binaries) != 1:
        raise RuntimeError('Expected one instrumented Cockpit library test executable.')
    profiles = Path(tempfile.mkdtemp(prefix='test-profiles-', dir=output))
    test_environment = dict(environment, LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw'))
    command([binaries[0]], root, test_environment, output / 'native-tests.log')
    sysroot = Path(command(['rustc', '--print', 'sysroot'], root).strip())
    host = re.search(r'^host: (.+)$', command(['rustc', '-vV'], root), re.M).group(1)
    llvm = sysroot / 'lib/rustlib' / host / 'bin'
    profile = output / 'coverage.profdata'
    command([str(llvm / 'llvm-profdata'), 'merge', '-sparse', *map(str, profiles.glob('*.profraw')),
             '-o', str(profile)], root)
    sources = [root / 'src-tauri/src/actions.rs', root / 'src-tauri/src/stm32.rs',
               root / 'src-tauri/src/pico.rs', root / 'src-tauri/src/local_gateway.rs']
    annotated = command([str(llvm / 'llvm-cov'), 'show', binaries[0], f'-instr-profile={profile}',
                         '-show-line-counts-or-regions', *map(str, sources)], root)
    (output / 'lines.txt').write_text(annotated)
    counts, current = {}, ''
    for line in annotated.splitlines():
        if line.endswith(':') and line[:-1] in map(str, sources):
            current = Path(line[:-1]).name
        if match := re.match(r'^\s*(\d+)\|\s*(\d+)\|', line):
            counts.setdefault(current, {})[int(match[1])] = int(match[2])
    actions = sources[0].read_text().splitlines()
    adapter = sources[1].read_text().splitlines()
    pico = sources[2].read_text().splitlines()
    local = sources[3].read_text().splitlines()
    find = lambda lines, marker: next(i + 1 for i, line in enumerate(lines) if marker in line)
    ranges = {'actions.rs': [(1, find(actions, '#[cfg(test)]') - 1)],
              'stm32.rs': [(find(adapter, 'fn action_route('), find(adapter, 'struct Gateway {') - 1),
                           (find(adapter, '    fn action_command('), find(adapter, '    /// An autonomous arrival') - 1),
                           (find(adapter, 'enum Role {'), find(adapter, 'fn number(') - 1),
                           (find(adapter, '    fn debug_command('), find(adapter, '    fn handle(') - 1)],
              'pico.rs': [(1, find(pico, '#[cfg(test)]') - 1)],
              'local_gateway.rs': [(1, find(local, '    #[cfg(test)]') - 1)]}
    report = {}
    for name, intervals in ranges.items():
        measured = {line: count for line, count in counts[name].items()
                    if any(start <= line <= end for start, end in intervals)}
        covered = sum(count > 0 for count in measured.values())
        percent = 100 * covered / len(measured)
        report[name] = {'covered': covered, 'lines': len(measured), 'percent': round(percent, 2)}
        print(f'{name}: {covered}/{len(measured)} production lines ({percent:.2f}%)', flush=True)
    (output / 'action-summary.json').write_text(json.dumps(report, indent=2) + '\n')
    if any(item['percent'] <= 70 for item in report.values()):
        raise SystemExit('Production device-action line coverage must exceed 70%.')


if __name__ == '__main__':
    main()
