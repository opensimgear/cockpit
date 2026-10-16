#!/usr/bin/env python3
"""Run native tests and gate each integration production module at least 71% production line coverage."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def command(args, cwd, env=None):
    return subprocess.check_output(args, cwd=cwd, env=env, text=True, encoding="utf-8", errors="replace")


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
    suffix = '.exe' if os.name == 'nt' else ''
    if not all((llvm / f'{tool}{suffix}').exists() for tool in ['llvm-profdata', 'llvm-cov']):
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
    tests = command([binaries[0], '--test-threads=1'], root,
                    dict(environment, LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw')))
    (output / 'native-tests.log').write_text(tests, encoding='utf-8')
    profile = output / 'coverage.profdata'
    command([str(llvm / f'llvm-profdata{suffix}'), 'merge', '-sparse', *map(str, profiles.glob('*.profraw')),
             '-o', str(profile)], root)
    sources = [root / 'src-tauri/src' / name
               for name in ['integration_profile.rs', 'integration_runtime.rs', 'game_telemetry.rs', 'xplane.rs', 'xplane_installation.rs',
                            'acr_windows.rs', 'output_leases.rs', 'runtime.rs', 'games.rs', 'game_discovery.rs', 'mapping_panels.rs', 'game_mapping.rs', 'game_companions.rs', 'companion_game_owner.rs', 'companion_feedback.rs']]
    annotated = command([str(llvm / f'llvm-cov{suffix}'), 'show', binaries[0], f'-instr-profile={profile}',
                         '-show-line-counts-or-regions', *map(str, sources)], root)
    (output / 'lines.txt').write_text(annotated, encoding='utf-8')
    source_paths = {str(path): path for path in sources}
    counts, current = {}, ''
    for line in annotated.splitlines():
        if line.endswith(':') and line[:-1] in source_paths:
            current = source_paths[line[:-1]].name
        if match := re.match(r'^\s*(\d+)\|\s*(\d+)\|', line):
            counts.setdefault(current, {})[int(match[1])] = int(match[2])
    report = {}
    for source in sources:
        lines = source.read_text(encoding='utf-8').splitlines()
        end = next((index + 1 for index, line in enumerate(lines) if '#[cfg(test)]' in line), len(lines) + 1)
        measured = {line: count for line, count in counts.get(source.name, {}).items() if line < end}
        if not measured:
            raise RuntimeError(f'No production coverage recorded for {source.name}.')
        covered = sum(count > 0 for count in measured.values())
        percent = 100 * covered / len(measured)
        report[source.name] = {'covered': covered, 'lines': len(measured), 'percent': round(percent, 2)}
        print(f'{source.name}: {covered}/{len(measured)} production lines ({percent:.2f}%)', flush=True)
    (output / 'integration-summary.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    if any(item['percent'] < 71 for item in report.values()):
        raise SystemExit('Each integration production module must meet 71% production line coverage.')


if __name__ == '__main__':
    main()
