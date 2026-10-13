#!/usr/bin/env python3
"""Clone, initialize and build external firmware, then test ordinary Cockpit."""
import argparse
import json
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from bootstrap import Dependencies, command, source_arguments


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', action='append', default=[], metavar='NAME=PATH', help='Clone an explicit local working-tree snapshot instead of the remote ref')
    parser.add_argument('--sanitize', action='store_true')
    parser.add_argument('--prepare-only', action='store_true', help='Build dependencies and print the launch manifest without starting tests')
    parser.add_argument('--output-dir', type=Path, help='Keep research checkouts, builds and evidence under this directory instead of .e2e')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    dependencies = Dependencies(root, source_arguments(args.source), args.output_dir)
    gateway = dependencies.acquire('device-gateway')
    device = dependencies.acquire('device-template')
    common = dependencies.common(gateway, device)
    build = dependencies.directory / 'build'
    for name, repository in [('device-gateway', gateway), ('device-template', device)]:
        command([sys.executable, repository / 'simulation/build.py', '--common-source', common,
                 '--output-dir', build / name, *(['--sanitize'] if args.sanitize else [])])
    command(['pnpm', 'install', '--frozen-lockfile'], cwd=root)
    environment = dict(os.environ, CARGO_TARGET_DIR=str(build / 'cockpit'))
    command(['cargo', 'build', '--locked', '--manifest-path', root / 'src-tauri/Cargo.toml', '--bin', 'osg-gateway-service'], env=environment)
    configuration = {
        'root': str(root), 'gatewayRepository': str(gateway), 'deviceRepository': str(device),
        'gatewayBinary': str(build / 'device-gateway/osg-gateway-sim'),
        'deviceBuild': str(build / 'device-template'), 'commonSource': str(common),
        'serviceBinary': str(build / 'cockpit/debug/osg-gateway-service'),
        'python': sys.executable, 'artifacts': str(dependencies.directory / 'runs'),
    }
    manifest = dependencies.directory / 'launch.json'
    manifest.write_text(json.dumps(configuration, indent=2) + '\n')
    print(f'Launch manifest: {manifest}', flush=True)
    if not args.prepare_only:
        command(['node', '--test', root / 'tests/e2e/acceptance.mjs'],
                env=dict(environment, OSG_E2E_MANIFEST=str(manifest)))


if __name__ == '__main__':
    main()
