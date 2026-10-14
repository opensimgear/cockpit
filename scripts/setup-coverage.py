#!/usr/bin/env python3
"""Run real setup paths and require at least 71% production line coverage per module."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def command(arguments, cwd, env=None):
    try:
        return subprocess.check_output(arguments, cwd=cwd, env=env, text=True)
    except subprocess.CalledProcessError as error:
        print(error.output, end='', flush=True)
        raise


def production_end(lines):
    """Top-level test items are trailing; indented fixture branches remain production."""
    return next((index + 1 for index, line in enumerate(lines)
                 if line.startswith('#[cfg(test)]')), len(lines) + 1)


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=root / '.research/setup-coverage')
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    archive = os.environ.get('OSG_DCS_SETUP_ARCHIVE')
    if not archive or not Path(archive).is_file():
        raise SystemExit('Set OSG_DCS_SETUP_ARCHIVE to the pinned official DCS-BIOS v0.11.7 archive; the smoke test verifies its checksum.')
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
    test_environment = dict(environment, LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw'))
    tests = command([binaries[0]], root, test_environment)
    smoke = command([binaries[0], 'official_pinned_archive_smoke', '--ignored'], root, test_environment)
    if not re.search(r'test result: ok\. [1-9]\d* passed;', smoke):
        raise RuntimeError('The official archive smoke test did not run; production runtime coverage cannot be qualified.')
    (output / 'native-tests.log').write_text(tests + '\n' + smoke)
    profile = output / 'coverage.profdata'
    command([str(llvm / 'llvm-profdata'), 'merge', '-sparse', *map(str, profiles.glob('*.profraw')),
             '-o', str(profile)], root)
    sources = [root / 'src-tauri/src' / name
               for name in ['dcs_setup.rs', 'game_integration_setup.rs', 'acr_setup.rs']]
    annotated = command([str(llvm / 'llvm-cov'), 'show', binaries[0], f'-instr-profile={profile}',
                         '-show-line-counts-or-regions', *map(str, sources)], root)
    (output / 'lines.txt').write_text(annotated)
    source_paths = {str(path): path for path in sources}
    counts, current = {}, ''
    for line in annotated.splitlines():
        if line.endswith(':') and line[:-1] in source_paths:
            current = source_paths[line[:-1]].name
        if match := re.match(r'^\s*(\d+)\|\s*(\d+(?:\.\d+)?[kMGT]?)\|', line):
            # llvm-cov abbreviates large hit counts; line coverage needs only zero/nonzero.
            counts.setdefault(current, {})[int(match[1])] = int(match[2] != '0')
    report = {}
    for source in sources:
        end = production_end(source.read_text().splitlines())
        measured = {line: count for line, count in counts.get(source.name, {}).items() if line < end}
        if not measured:
            raise RuntimeError(f'No production coverage recorded for {source.name}.')
        covered = sum(count > 0 for count in measured.values())
        percent = 100 * covered / len(measured)
        report[source.name] = {'covered': covered, 'lines': len(measured), 'percent': round(percent, 2)}
        print(f'{source.name}: {covered}/{len(measured)} production lines ({percent:.2f}%)', flush=True)
    (output / 'setup-summary.json').write_text(json.dumps(report, indent=2) + '\n')
    if any(item['covered'] * 100 < 71 * item['lines'] for item in report.values()):
        raise SystemExit('Each setup production module must have at least 71% line coverage.')


if __name__ == '__main__':
    main()
