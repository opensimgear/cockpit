#!/usr/bin/env python3
"""Run native tests and gate each integration production module at least 71% production line coverage."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from native_coverage_sources import measure_module


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
    paths.update(root / name for name in ['src-tauri/build.rs', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'rust-toolchain.toml'] if (root / name).is_file())
    paths.update(root / 'scripts' / name for name in ['integration-coverage.py', 'native_coverage_sources.py', 'test_native_coverage_sources.py', 'desktop-output-coverage.py'] if (root / 'scripts' / name).is_file())
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


def test_only_lines(source):
    spec = importlib.util.spec_from_file_location('desktop_coverage_source', Path(__file__).with_name('desktop-output-coverage.py'))
    parser = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(parser)
    return parser.test_only_lines(source)


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    module_names = ['game_telemetry.rs', 'xplane.rs', 'xplane_installation.rs', 'games/telemetry/windows_source/mod.rs', 'games/telemetry/windows_source/platform.rs', 'output_leases.rs', 'runtime.rs', 'games.rs', 'games/storage/installations.rs', 'game_discovery.rs', 'games/artwork/mod.rs', 'games/artwork/sources.rs', 'games/artwork/discovery_cache.rs', 'games/artwork/steam_online/mod.rs', 'games/artwork/steam_online/http.rs', 'games/artwork/steam_online/targets.rs', 'games/artwork/model.rs', 'games/library/mod.rs', 'game_metadata_api.rs', 'companion_game_owner.rs', 'retained_game_companions.rs', 'persistence/paths.rs', 'profile_retired_game_io.rs', 'companion/storage/widgets.rs', 'companion/storage/workspace.rs', 'companion/storage/workspace_validation.rs', 'mcp_tools/workspace.rs', 'companion/storage/host_settings.rs', 'mcp_tools/host_settings.rs', 'mcp/server/mod.rs', 'mcp/server/auth.rs', 'mcp/server/credentials.rs', 'mcp/server/transport.rs', 'mcp/storage/credentials.rs', 'mcp_tools/integration_settings.rs']
    module_names.extend(['games/artwork/installation_cache.rs', 'profile_session_journal.rs', 'profile_retired_recovery.rs', 'persistence/development.rs','persistence/backup/mod.rs','persistence/backup/model.rs','persistence/backup/assets.rs','persistence/backup/startup.rs','persistence/backup/gateway.rs','persistence/backup/trust.rs'])
    module_names.extend(['persistence/ui/mod.rs', 'persistence/ui/validation.rs', 'persistence/ui/window_writer.rs', 'persistence/ui/commands.rs', 'window_persistence.rs'])
    module_names.extend(['activity/mod.rs', 'activity/capture.rs', 'activity/audit.rs', 'activity/changes.rs', 'activity/context.rs', 'activity/messages.rs', 'activity/diagnostics.rs', 'activity/gateway.rs', 'activity/writer.rs', 'activity/session.rs', 'activity/inventory.rs', 'activity/games.rs', 'activity/integration.rs', 'activity/effects.rs', 'activity/observations.rs', 'activity/reconcile.rs', 'activity/scopes.rs'])
    module_names.extend(['activity/metadata.rs', 'activity/sources.rs', 'system_inventory.rs',
                         'gateways/serial_topology.rs', 'gateways/workspace/publisher.rs', 'companion/host/mod.rs'])
    module_names.extend(['profile_session/companion_scheduling.rs', 'profile_session/companion_feedback.rs',
                         'profile_session/companion_feedback/delivery.rs',
                         'profile_session/companion_pending.rs', 'profile_session/companion_rebind.rs',
                         'profile_session_companions.rs', 'profile_session_companion_host.rs'])
    module_names.extend(['dcs_physics.rs', 'dcs_physics/parser.rs', 'dcs_physics/session.rs', 'dcs_physics/source.rs', 'dcs_physics/retained.rs'])
    module_names.extend(['dcs_bios/transport.rs'])
    module_names.append('game_telemetry/capture.rs')
    module_names.append('game_telemetry/session.rs')
    module_names.extend(['game_telemetry_inspection.rs', 'games/telemetry/windows_source/shared.rs'])
    module_names.extend(['native_capture/mod.rs','game_channels/captured.rs',
                         'dcs_profile/captured_values.rs','game_telemetry/project.rs',
                         'dcs_physics/project.rs','xplane/snapshot.rs'])
    module_names.extend(['dcs_bios/transport_client.rs', 'dcs_bios/decoded/mod.rs',
                         'dcs_bios/parser.rs', 'dcs_bios/decoded/captured_memory.rs', 'dcs_bios/decoded/state.rs', 'dcs_bios/decoded/source.rs', 'dcs_profile/shared.rs'])
    module_names.append('xplane/subscriptions.rs')
    module_names.extend(['profile_session_games.rs', 'games/profiles/session_prepare.rs',
                         'games/profiles/session_read.rs', 'games/profiles/native_admission.rs',
                         'games/profiles/native_tables.rs', 'profile_session_host.rs',
                         'profile_session_launch_host.rs', 'profile_session_desktop.rs',
                         'profile_session/host_scheduling.rs',
                         'profile_session/activation_heartbeat.rs'])
    module_names.extend(['profile_session/source_admission.rs', 'games/profiles/session_delivery.rs', 'games/vehicles/sources.rs'])
    module_names.extend(['games/profiles/source_wakes.rs', 'games/vehicles/views.rs', 'game_telemetry_api.rs', 'gateways.rs'])
    module_names.extend(['profile_read_models/mod.rs', 'profile_read_models/bounds.rs',
                         'profile_read_models/emergency.rs', 'profile_read_models/pages.rs'])
    module_names.extend(['xplane/owner.rs', 'xplane/owner/client.rs', 'xplane/owner/worker.rs',
                         'xplane/permissions.rs', 'games/process_monitor.rs', 'gateways/process_monitor.rs', 'mcp_tools.rs',
                         'profile_session/scheduling.rs', 'profile_session_scripts.rs',
                         'profile_session_inputs.rs', 'profile_session/activation_phases.rs'])
    module_names.extend(['profile_samples/mod.rs', 'profile_samples/registration.rs', 'profile_samples/worker.rs',
                         'profile_session/samples/schema.rs', 'profile_session/samples/coordinator.rs',
                         'gateways/profile_samples.rs', 'observations/mod.rs', 'observations/service.rs'])
    module_names.extend(['games/artwork/steam_online/owner.rs', 'games/review/worker.rs'])
    module_names.extend(['games/integration/mod.rs', 'games/integration/setup_cache.rs',
                         'games/integration/provider_settings.rs', 'games/integration/requests.rs',
                         'games/integration/provision.rs', 'games/integration/settings.rs',
                         'games/integration/catalogues.rs', 'games/integration/inspection.rs',
                         'game_context_runtime.rs'])
    module_names.extend(['games/artwork/prepared.rs', 'games/prepared_sources/mod.rs',
                         'games/prepared_sources/watch.rs', 'games/storage/car_catalogue.rs',
                         'games/integration/car_publication.rs'])
    module_names.extend(['game_read_models/signals.rs', 'game_read_models/pages.rs',
                         'game_read_models/codec.rs', 'game_read_models/details.rs',
                         'game_read_models/runtime.rs', 'games/read_model.rs',
                         'games/vehicles/mod.rs', 'games/vehicles/polling.rs',
                         'games/vehicles/observation.rs', 'games/review/mod.rs',
                         'games/review/completion.rs', 'game_activation.rs', 'game_context.rs'])
    if os.name == 'nt':
        module_names.append('profile_session/desktop_foreground.rs')
        module_names.extend(['xplane/process_events.rs', 'xplane/process_events/native.rs',
                             'xplane/process_events/windows.rs', 'xplane/process_events/sink.rs',
                             'xplane/process_events/bridge.rs', 'xplane/process_events/bridge/protocol.rs',
                             'xplane/process_events/bridge/security.rs', 'xplane/process_events/bridge/wait.rs',
                             'xplane/process_events/bridge/launch.rs'])
    module_names.extend(['stm32/publication_feed.rs', 'stm32_panel_serial.rs',
                         'stm32_publication.rs', 'profile_publication_actor.rs'])
    module_names.extend(['stm32/inventory_service.rs', 'stm32/workspace.rs',
                         'profile_hid_publication.rs', 'games/profiles/inventory_collection.rs'])
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
    report, annotations = {}, []
    for source in sources:
        result, annotated = measure_module(source, lambda path: command([
            str(llvm / f'llvm-cov{suffix}'), 'show', binaries[0], f'-instr-profile={profile}',
            '-show-line-counts-or-regions', str(path)], root), root)
        report[str(source.relative_to(root))] = result
        annotations.append(annotated)
        print(f"{source.name}: {result['covered']}/{result['lines']} production lines ({result['percent']:.2f}%)", flush=True)
    (output / 'lines.txt').write_text('\n'.join(annotations), encoding='utf-8')
    (output / 'integration-summary.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    (output / 'evidence.json').write_text(json.dumps({'testBinary': binaries[0], 'testInvocations': invocations, 'profile': str(profile), 'sources': final_stamp, 'measuredModules': [source.name for source in sources]}, indent=2) + '\n', encoding='utf-8')
    if any(item['covered'] * 100 < item['lines'] * 71 for item in report.values()):
        raise SystemExit('Each integration production module must meet 71% production line coverage.')


if __name__ == '__main__':
    main()
