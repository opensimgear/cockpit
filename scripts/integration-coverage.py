#!/usr/bin/env python3
"""Run native tests and gate each integration production module above 70% line coverage."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def command(args, cwd, env=None):
    return subprocess.check_output(args, cwd=cwd, env=env, text=True)


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path,
                        default=root.parent / '.research/integration-coverage')
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    sysroot = Path(command(['rustc', '--print', 'sysroot'], root).strip())
    host = re.search(r'^host: (.+)$', command(['rustc', '-vV'], root), re.M).group(1)
    llvm = sysroot / 'lib/rustlib' / host / 'bin'
    if not all((llvm / tool).exists() for tool in ['llvm-profdata', 'llvm-cov']):
        raise SystemExit('Install the pinned Rust llvm-tools-preview component before measuring coverage.')
    build_profiles = output / 'build-profiles'
    build_profiles.mkdir(exist_ok=True)
    environment = dict(os.environ,
                       CARGO_TARGET_DIR=os.environ.get('CARGO_TARGET_DIR', str(output / 'target')),
                       RUSTFLAGS='-C instrument-coverage',
                       LLVM_PROFILE_FILE=str(build_profiles / '%m-%p.profraw'))
    artifacts = command(['cargo', 'test', '--locked', '--lib', '--no-run', '--message-format=json',
                         '--manifest-path', str(root / 'src-tauri/Cargo.toml')], root, environment)
    binaries = [entry['executable'] for line in artifacts.splitlines()
                if (entry := json.loads(line)).get('reason') == 'compiler-artifact'
                and entry.get('executable') and entry.get('profile', {}).get('test')]
    if len(binaries) != 1:
        raise RuntimeError('Expected one instrumented Cockpit library test executable.')
    profiles = Path(tempfile.mkdtemp(prefix='test-profiles-', dir=output))
    tests = command([binaries[0]], root,
                    dict(environment, LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw')))
    (output / 'native-tests.log').write_text(tests)
    profile = output / 'coverage.profdata'
    command([str(llvm / 'llvm-profdata'), 'merge', '-sparse', *map(str, profiles.glob('*.profraw')),
             '-o', str(profile)], root)
    sources = [root / 'src-tauri/src' / name
               for name in ['integration_profile.rs', 'integration_runtime.rs', 'game_telemetry.rs',
                            'acr_windows.rs', 'output_leases.rs', 'runtime.rs']]
    annotated = command([str(llvm / 'llvm-cov'), 'show', binaries[0], f'-instr-profile={profile}',
                         '-show-line-counts-or-regions', *map(str, sources)], root)
    (output / 'lines.txt').write_text(annotated)
    source_paths = {str(path): path for path in sources}
    counts, current = {}, ''
    for line in annotated.splitlines():
        if line.endswith(':') and line[:-1] in source_paths:
            current = source_paths[line[:-1]].name
        if match := re.match(r'^\s*(\d+)\|\s*(\d+)\|', line):
            counts.setdefault(current, {})[int(match[1])] = int(match[2])
    report = {}
    for source in sources:
        lines = source.read_text().splitlines()
        end = next((index + 1 for index, line in enumerate(lines) if '#[cfg(test)]' in line), len(lines) + 1)
        measured = {line: count for line, count in counts.get(source.name, {}).items() if line < end}
        if not measured:
            raise RuntimeError(f'No production coverage recorded for {source.name}.')
        covered = sum(count > 0 for count in measured.values())
        percent = 100 * covered / len(measured)
        report[source.name] = {'covered': covered, 'lines': len(measured), 'percent': round(percent, 2)}
        print(f'{source.name}: {covered}/{len(measured)} production lines ({percent:.2f}%)', flush=True)
    (output / 'integration-summary.json').write_text(json.dumps(report, indent=2) + '\n')
    if any(item['percent'] <= 70 for item in report.values()):
        raise SystemExit('Each integration production module must exceed 70% line coverage.')


if __name__ == '__main__':
    main()
