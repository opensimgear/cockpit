#!/usr/bin/env python3
"""Run native tests and gate each integration production module at least 71% production line coverage."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def source_stamp(root):
    # The full native suite depends on shared provider/engine/journal contracts.
    # Pin sources and tests, manifests and locks rather than claim an unstamped
    # aggregate after a concurrent edit during a long-running instrumented suite.
    paths = set((root / 'src-tauri/src').rglob('*.rs'))
    paths.update((root / 'src-tauri/src').rglob('*.sql'))
    for crate in (root / 'crates').iterdir():
        if crate.is_dir() and (crate / 'src').is_dir():
            paths.update((crate / 'src').rglob('*.rs'))
            paths.update((crate / 'src').rglob('*.sql'))
            paths.update(path for path in [crate / 'Cargo.toml', crate / 'Cargo.lock'] if path.is_file())
    paths.update(root / name for name in ['src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'rust-toolchain.toml'] if (root / name).is_file())
    return {str(path.relative_to(root)): {'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'modifiedNs': path.stat().st_mtime_ns} for path in sorted(paths)}


def command(args, cwd, env=None):
    return subprocess.check_output(args, cwd=cwd, env=env, text=True, encoding="utf-8", errors="replace")


def production_end(lines):
    # A test-only import near the top does not end the production module.
    # Keep the existing exclusion for the test/helper section itself.
    for index, line in enumerate(lines):
        if line == '#[cfg(test)]':
            following = next((entry.strip() for entry in lines[index + 1:] if entry.strip()), '')
            if not following.startswith(('use ', 'pub use ')):
                return index + 1
    return len(lines) + 1


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    module_names = ['game_telemetry.rs', 'xplane.rs', 'xplane_installation.rs', 'games/telemetry/windows_source/mod.rs', 'games/telemetry/windows_source/platform.rs', 'output_leases.rs', 'runtime.rs', 'games.rs', 'game_cleanup.rs', 'games/storage/installations.rs', 'game_discovery.rs', 'games/artwork/mod.rs', 'games/artwork/sources.rs', 'games/artwork/model.rs', 'game_library.rs', 'game_metadata_api.rs', 'companion_game_owner.rs', 'retained_game_companions.rs', 'persistence/paths.rs', 'profile_retired_game_io.rs', 'companion/storage/widgets.rs']
    parser.add_argument('--module', action='append', choices=module_names, help='Focused local coverage; CI measures every module.')
    parser.add_argument('--test-filter', action='append', default=[])
    parser.add_argument('--output-dir', type=Path,
                        default=root.parent / '.research/integration-coverage')
    parser.add_argument('--test-threads', type=int, choices=range(1, 33), default=1,
                        help='Native regression workers; CI keeps its existing single-worker default.')
    args = parser.parse_args()
    if args.test_filter and not args.module:
        parser.error('Focused test filters require an explicit module selection')
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for name in ['evidence.json', 'native-tests.log', 'integration-summary.json']:
        (output / name).unlink(missing_ok=True)
    initial_stamp = source_stamp(root)
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
    invocations = [[binaries[0], *([test_filter] if test_filter else []), f'--test-threads={args.test_threads}']
                   for test_filter in args.test_filter or ['']]
    test_environment = dict(environment, LLVM_PROFILE_FILE=str(profiles / '%m-%p.profraw'))
    with (output / 'native-tests.log').open('w', encoding='utf-8') as log:
        for arguments in invocations:
            result = subprocess.run(arguments, cwd=root, env=test_environment, stdout=log, stderr=subprocess.STDOUT)
            log.flush()
            if result.returncode:
                print((output / 'native-tests.log').read_text(encoding='utf-8'), flush=True)
                result.check_returncode()
    final_stamp = source_stamp(root)
    if initial_stamp != final_stamp:
        changed = sorted(name for name in set(initial_stamp) | set(final_stamp) if initial_stamp.get(name) != final_stamp.get(name))
        raise RuntimeError(f'Sources or tests changed during coverage; rerun: {changed}')
    profile = output / 'coverage.profdata'
    command([str(llvm / f'llvm-profdata{suffix}'), 'merge', '-sparse', *map(str, profiles.glob('*.profraw')),
             '-o', str(profile)], root)
    sources = [root / 'src-tauri/src' / name for name in args.module or module_names]
    sources = [source for source in sources if not (source.name == "platform.rs" and os.name != "nt")]
    annotated = command([str(llvm / f'llvm-cov{suffix}'), 'show', binaries[0], f'-instr-profile={profile}',
                         '-show-line-counts-or-regions', *map(str, sources)], root)
    (output / 'lines.txt').write_text(annotated, encoding='utf-8')
    source_paths = {str(path).replace('\\', '/'): path for path in sources}
    counts, current = {}, ''
    for line in annotated.splitlines():
        heading = line[:-1].replace('\\', '/') if line.endswith(':') else ''
        if heading in source_paths:
            current = str(source_paths[heading].relative_to(root))
        if match := re.match(r'^\s*(\d+)\|\s*([0-9]+(?:\.[0-9]+)?[kMGT]?)\s*\|', line):
            counts.setdefault(current, {})[int(match[1])] = float(match[2].rstrip('kMGT'))
    report = {}
    for source in sources:
        lines = source.read_text(encoding='utf-8').splitlines()
        end = production_end(lines)
        measured = {line: count for line, count in counts.get(str(source.relative_to(root)), {}).items() if line < end}
        if not measured:
            raise RuntimeError(f'No production coverage recorded for {source.name}.')
        covered = sum(count > 0 for count in measured.values())
        percent = 100 * covered / len(measured)
        report[str(source.relative_to(root))] = {'covered': covered, 'lines': len(measured), 'percent': round(percent, 2)}
        print(f'{source.name}: {covered}/{len(measured)} production lines ({percent:.2f}%)', flush=True)
    (output / 'integration-summary.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    (output / 'evidence.json').write_text(json.dumps({'testBinary': binaries[0], 'testInvocations': invocations, 'profile': str(profile), 'sources': final_stamp, 'measuredModules': [source.name for source in sources]}, indent=2) + '\n', encoding='utf-8')
    if any(item['percent'] < 71 for item in report.values()):
        raise SystemExit('Each integration production module must meet 71% production line coverage.')


if __name__ == '__main__':
    main()
