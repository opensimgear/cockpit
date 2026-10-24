#!/usr/bin/env python3
"""Exercise native Windows process discovery with bounded, owned GUI fixtures."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import uuid


def source_stamp(root, fixture):
    paths = set((root / 'crates/profiles/src').rglob('*.rs'))
    paths.update(root / 'src-tauri/src' / name for name in
                 ['game_discovery.rs', 'game_activation.rs', 'xplane_installation.rs',
                  'games/storage/installations.rs', 'games/artwork/mod.rs',
                  'games/artwork/model.rs', 'games/artwork/sources.rs'])
    paths.update(fixture / name for name in ['Cargo.toml', 'Cargo.lock', 'main.rs', 'stub.rs'])
    paths.update(root / name for name in ['crates/profiles/Cargo.toml', 'scripts/windows-process-smoke.py'])
    return {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(paths)}


def run(command, root, environment, log):
    with log.open('w', encoding='utf-8') as output:
        output.write(json.dumps(command) + '\n')
        output.flush()
        subprocess.run(command, cwd=root, env=environment, stdout=output,
                       stderr=subprocess.STDOUT, check=True,
                       creationflags=subprocess.CREATE_NO_WINDOW)


def main():
    if sys.platform != 'win32':
        raise SystemExit('Run this fixture on native Windows; it exercises real Windows process APIs.')
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, help='New evidence directory; defaults below .research.')
    args = parser.parse_args()
    output = (args.output_dir or root / '.research/windows-process-smoke' /
              (datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + str(uuid.uuid4()))).resolve()
    output.mkdir(parents=True, exist_ok=False)
    fixture = root / 'scripts/fixtures/windows-process-smoke'
    before = source_stamp(root, fixture)
    (output / 'source-hashes.before.json').write_text(json.dumps(before, indent=2) + '\n', encoding='utf-8')
    environment = dict(os.environ, CARGO_TARGET_DIR=str(output / 'target'))
    # Keep this tiny build independent of any instrumented shared native build.
    for name in ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'LLVM_PROFILE_FILE']:
        environment.pop(name, None)
    try:
        run(['cargo', 'build', '--offline', '--locked', '--bins', '--manifest-path',
             str(fixture / 'Cargo.toml')], root, environment, output / 'build.log')
        artifacts = output / 'artifacts'
        artifacts.mkdir()
        hashes = {}
        for name in ['native-process-smoke.exe', 'DCS.exe']:
            source = output / 'target/debug' / name
            target = artifacts / name
            shutil.copyfile(source, target)
            hashes[name] = hashlib.sha256(target.read_bytes()).hexdigest()
            if hashes[name] != hashlib.sha256(source.read_bytes()).hexdigest():
                raise RuntimeError(f'Copied executable changed: {name}')
        run([str(artifacts / 'native-process-smoke.exe'), str(artifacts / 'DCS.exe'), str(output)],
            root, environment, output / 'run.log')
        result = json.loads((output / 'result.json').read_text(encoding='utf-8'))
        if result['result'] != 'pass' or not result['helpersStopped']:
            raise RuntimeError('Native process fixture did not complete and stop its owned helpers.')
        for name, digest in hashes.items():
            if hashlib.sha256((artifacts / name).read_bytes()).hexdigest() != digest:
                raise RuntimeError(f'Executed artifact changed: {name}')
    finally:
        after = source_stamp(root, fixture)
        (output / 'source-hashes.after.json').write_text(json.dumps(after, indent=2) + '\n', encoding='utf-8')
        if before != after:
            raise RuntimeError('Production or fixture sources changed during the smoke; rerun.')
    evidence = {'schema': 1, 'result': 'pass', 'scope': result['scope'],
                'sourceHashesUnchanged': True, 'sourceHashes': before,
                'artifacts': hashes, 'target': str(output / 'target'),
                'logs': ['build.log', 'run.log'], 'details': 'result.json'}
    (output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n', encoding='utf-8')
    print(f"Windows process smoke passed: {output}")


if __name__ == '__main__':
    main()
