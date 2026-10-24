#!/usr/bin/env python3
"""Run real setup paths and require at least 71% production line coverage per module."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def command(arguments, cwd, env=None):
    try:
        return subprocess.check_output(arguments, cwd=cwd, env=env, text=True, encoding='utf-8', errors='replace')
    except subprocess.CalledProcessError as error:
        print(error.output, end='', flush=True)
        raise


def production_end(lines):
    """Top-level test items are trailing; indented fixture branches remain production."""
    for index, line in enumerate(lines):
        if line == '#[cfg(test)]':
            following = next((entry.strip() for entry in lines[index + 1:] if entry.strip()), '')
            if not following.startswith(('use ', 'pub use ')):
                return index + 1
    return len(lines) + 1


def main():
    root = Path(__file__).resolve().parents[1]
    module_names = ['dcs_setup.rs', 'dcs_setup/aircraft.rs', 'games/integration/mod.rs', 'acr_setup.rs',
                    'game_context.rs', 'game_context_runtime.rs', 'games/integration/provider_settings.rs', 'games/integration/settings.rs',
                    'games/integration/requests.rs', 'games/integration/provision.rs',
                    'games/integration/inspection.rs', 'games/integration/catalogues.rs',
                    'profile_session.rs', 'profile_session_journal.rs', 'profile_session_games.rs',
                    'profile_session_stage.rs', 'profile_session_inputs.rs',
                    'profile_session_scripts.rs', 'profile_session_virtual.rs',
                    'profile_hid_publication.rs', 'profile_session_dcs_hid.rs',
                    'games/storage/car_catalogue.rs', 'games/vehicles/mod.rs', 'games/vehicles/sources.rs', 'profile_desktop_settings.rs']
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=root / '.research/setup-coverage')
    parser.add_argument('--module', action='append', choices=module_names,
                        help='Focused local measurement; CI defaults to every production module.')
    parser.add_argument('--test-filter', action='append', default=[],
                        help='Focused local test filter; CI defaults to the complete native suite.')
    args = parser.parse_args()
    sources = [root / 'src-tauri/src' / name for name in args.module or module_names]
    source_hashes = {str(source.relative_to(root)): hashlib.sha256(source.read_bytes()).hexdigest()
                     for source in sources}
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    archive = os.environ.get('OSG_DCS_SETUP_ARCHIVE')
    if not archive or not Path(archive).is_file():
        raise SystemExit('Set OSG_DCS_SETUP_ARCHIVE to the pinned official DCS-BIOS v0.11.7 archive; the smoke test verifies its checksum.')
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
    test_environment = dict(environment, LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw'))
    tests = ''.join(command([binaries[0], *([test_filter] if test_filter else []), '--test-threads=1'],
                            root, test_environment) for test_filter in args.test_filter or [''])
    smoke_logs = []
    for smoke_filter in ['official_pinned_archive_smoke',
                         'pinned_official_archive_commondata_normalization_and_native_guard']:
        smoke = command([binaries[0], smoke_filter, '--ignored', '--test-threads=1'], root, test_environment)
        (output / f'{smoke_filter}.log').write_text(smoke, encoding='utf-8')
        if not re.search(r'test result: ok\. [1-9]\d* passed;', smoke):
            raise RuntimeError(f'The required official archive smoke test {smoke_filter} did not run.')
        smoke_logs.append(smoke)
    (output / 'native-tests.log').write_text(tests + '\n' + '\n'.join(smoke_logs), encoding='utf-8')
    profile = output / 'coverage.profdata'
    command([str(llvm / f'llvm-profdata{suffix}'), 'merge', '-sparse', *map(str, profiles.glob('*.profraw')),
             '-o', str(profile)], root)
    if source_hashes != {str(source.relative_to(root)): hashlib.sha256(source.read_bytes()).hexdigest()
                         for source in sources}:
        raise RuntimeError('Setup/context sources changed during coverage; rebuild and rerun the gate.')
    annotated = command([str(llvm / f'llvm-cov{suffix}'), 'show', binaries[0], f'-instr-profile={profile}',
                         '-show-line-counts-or-regions', *map(str, sources)], root)
    (output / 'lines.txt').write_text(annotated, encoding='utf-8')
    source_paths = {str(path).replace('\\', '/'): path for path in sources}
    # llvm-cov omits the file heading when exactly one source was requested.
    counts, current = {}, str(sources[0].relative_to(root)) if len(sources) == 1 else ''
    for line in annotated.splitlines():
        heading = line[:-1].replace('\\', '/') if line.endswith(':') else ''
        if heading in source_paths:
            current = str(source_paths[heading].relative_to(root))
        if match := re.match(r'^\s*(\d+)\|\s*(\d+(?:\.\d+)?[kMGT]?)\|', line):
            # llvm-cov abbreviates large hit counts; line coverage needs only zero/nonzero.
            counts.setdefault(current, {})[int(match[1])] = int(match[2] != '0')
    report = {}
    for source in sources:
        end = production_end(source.read_text(encoding='utf-8').splitlines())
        measured = {line: count for line, count in counts.get(str(source.relative_to(root)), {}).items() if line < end}
        if not measured:
            raise RuntimeError(f'No production coverage recorded for {source.name}.')
        covered = sum(count > 0 for count in measured.values())
        percent = 100 * covered / len(measured)
        report[str(source.relative_to(root))] = {'covered': covered, 'lines': len(measured), 'percent': round(percent, 2)}
        print(f'{source.name}: {covered}/{len(measured)} production lines ({percent:.2f}%)', flush=True)
    (output / 'setup-summary.json').write_text(json.dumps(report, indent=2) + '\n')
    (output / 'source-hashes.json').write_text(json.dumps(source_hashes, indent=2) + '\n', encoding='utf-8')
    if any(item['covered'] * 100 < 71 * item['lines'] for item in report.values()):
        raise SystemExit('Each setup production module must have at least 71% line coverage.')


if __name__ == '__main__':
    main()
