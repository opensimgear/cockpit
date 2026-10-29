#!/usr/bin/env python3
"""Run real setup paths and require at least 71% production line coverage per module."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from native_coverage_sources import measure_module, source_stamp


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
    module_names = ['dcs_setup.rs', 'dcs_setup/aircraft.rs', 'dcs_setup/physics.rs', 'dcs_setup/recovery/mod.rs', 'dcs_setup/recovery/record.rs', 'games/integration/mod.rs', 'acr_setup.rs',
                    'game_context.rs', 'game_context_runtime.rs', 'games/integration/provider_settings.rs', 'games/integration/settings.rs',
                    'games/integration/requests.rs', 'games/integration/provision.rs',
                    'games/integration/inspection.rs', 'games/integration/catalogues.rs',
                    'profile_debug_controller.rs', 'profile_resources/game_retention.rs', 'profile_session.rs', 'profile_session_journal.rs', 'profile_session_games.rs',
                    'profile_session_stage.rs', 'profile_session_inputs.rs',
                    'profile_session_scripts.rs', 'profile_session_virtual.rs',
                    'profile_hid_publication.rs', 'profile_session_dcs_hid.rs',
                    'games/storage/car_catalogue.rs', 'games/vehicles/mod.rs', 'games/vehicles/observation.rs', 'games/vehicles/polling.rs',
                    'games/vehicles/sources.rs', 'games/vehicles/views.rs', 'profile_desktop_settings.rs']
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=root / '.research/setup-coverage')
    parser.add_argument('--module', action='append', choices=module_names,
                        help='Focused local measurement; CI defaults to every production module.')
    parser.add_argument('--test-filter', action='append', default=[],
                        help='Focused local test filter; CI defaults to the complete native suite.')
    args = parser.parse_args()
    sources = [root / 'src-tauri/src' / name for name in args.module or module_names]
    tracked = sources + [Path(__file__).resolve(), root / 'scripts/native_coverage_sources.py',
                         root / 'scripts/test_native_coverage_sources.py', root / 'scripts/desktop-output-coverage.py']
    source_hashes = source_stamp(tracked, root)
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
    if source_hashes != source_stamp(tracked, root):
        raise RuntimeError('Setup/context sources changed during coverage; rebuild and rerun the gate.')
    report, annotations = {}, []
    for source in sources:
        result, annotated = measure_module(source, lambda path: command([
            str(llvm / f'llvm-cov{suffix}'), 'show', binaries[0], f'-instr-profile={profile}',
            '-show-line-counts-or-regions', str(path)], root), root)
        report[str(source.relative_to(root))] = result
        annotations.append(annotated)
        print(f"{source.name}: {result['covered']}/{result['lines']} production lines ({result['percent']:.2f}%)", flush=True)
    (output / 'lines.txt').write_text('\n'.join(annotations), encoding='utf-8')
    (output / 'setup-summary.json').write_text(json.dumps(report, indent=2) + '\n')
    (output / 'source-hashes.json').write_text(json.dumps(source_hashes, indent=2) + '\n', encoding='utf-8')
    if any(item['covered'] * 100 < 71 * item['lines'] for item in report.values()):
        raise SystemExit('Each setup production module must have at least 71% line coverage.')


if __name__ == '__main__':
    main()
