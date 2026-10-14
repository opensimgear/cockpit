#!/usr/bin/env python3
"""Stage the optional Apple helper for release review; never sign or install it."""
import argparse
import hashlib
import json
from pathlib import Path
import os
import platform
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=ROOT / '.research/apple-helper')
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('Staging requires a native macOS build host and Apple SDK.')
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(output / 'target'))
    subprocess.run(['cargo', 'build', '--locked', '--release', '--manifest-path',
                    str(ROOT / 'crates/virtual-controller/Cargo.toml'), '--bin',
                    'osg-macos-hid-helper'], env=env, cwd=ROOT, check=True)
    payload = output / 'payload/Library/Application Support/OpenSimGear/Cockpit/HID'
    payload.mkdir(parents=True, exist_ok=True)
    helper = payload / 'osg-macos-hid-helper'
    shutil.copy2(output / 'target/release/osg-macos-hid-helper', helper)
    helper.chmod(0o755)
    result = subprocess.run([str(helper), '--inspect'], capture_output=True, text=True,
                            timeout=5, check=True)
    if json.loads(result.stdout)['qualified'] is not False:
        raise RuntimeError('Staging must not silently qualify an Apple helper.')
    shutil.copy2(ROOT / 'packaging/macos-hid/entitlements.plist', output / 'entitlements.plist')
    evidence = dict(qualified=False, signing='not performed', notarization='not performed',
                    architecture=platform.machine(), buildOS=platform.mac_ver()[0],
                    sha256=hashlib.sha256(helper.read_bytes()).hexdigest(),
                    helper=str(helper.relative_to(output)))
    (output / 'qualification.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))

if __name__ == '__main__':
    main()
