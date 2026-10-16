#!/usr/bin/env python3
"""Collect explicit mobile artifacts and sign Android releases without persisting keys."""
import base64
import hashlib
import os
import plistlib
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / 'apps/companion/src-tauri'
OUT = ROOT / '.research/mobile-release'


def exactly_one(paths):
    paths = sorted(paths)
    if len(paths) != 1:
        raise ValueError(f'Expected one artifact, found {paths}')
    return paths[0]


def main(command, *args):
    OUT.mkdir(parents=True, exist_ok=True)
    if command == 'android-debug':
        apk = exactly_one((APP / 'gen/android/app/build/outputs/apk').rglob('*debug*.apk'))
        shutil.copy2(apk, OUT / 'mobile-companion-android-arm64-debug.apk')
    elif command == 'android-sign':
        for key in ['ANDROID_KEYSTORE_BASE64', 'ANDROID_KEYSTORE_PASSWORD', 'ANDROID_KEY_ALIAS', 'ANDROID_KEY_PASSWORD']:
            if not os.environ.get(key):
                raise ValueError(f'Missing signing secret: {key}')
        apk = exactly_one((APP / 'gen/android/app/build/outputs/apk').rglob('*release-unsigned.apk'))
        tools = Path(os.environ['ANDROID_HOME']) / 'build-tools/36.0.0'
        with tempfile.TemporaryDirectory() as tmp:
            key = Path(tmp) / 'release.jks'
            key.write_bytes(base64.b64decode(os.environ['ANDROID_KEYSTORE_BASE64'], validate=True))
            key.chmod(0o600)
            aligned = Path(tmp) / 'aligned.apk'
            subprocess.run([str(tools / 'zipalign'), '-f', '-p', '4', str(apk), str(aligned)], check=True)
            output = OUT / 'mobile-companion-android-arm64-release.apk'
            subprocess.run([str(tools / 'apksigner'), 'sign', '--ks', str(key), '--ks-key-alias', os.environ['ANDROID_KEY_ALIAS'],
                            '--ks-pass', 'env:ANDROID_KEYSTORE_PASSWORD', '--key-pass', 'env:ANDROID_KEY_PASSWORD',
                            '--out', str(output), str(aligned)], check=True)
            subprocess.run([str(tools / 'apksigner'), 'verify', '--verbose', str(output)], check=True)
    elif command == 'ios-simulator':
        app = exactly_one(p for p in (APP / 'gen/apple/build/arm64-sim').glob('*.app') if p.is_dir())
        info = plistlib.loads((app / 'Info.plist').read_bytes())
        if 'iPhoneSimulator' not in info.get('CFBundleSupportedPlatforms', []):
            raise ValueError('Expected an iOS simulator app, not a device bundle')
        with tarfile.open(OUT / 'mobile-companion-ios-arm64-simulator.app.tar.gz', 'w:gz') as tar:
            tar.add(app, arcname=app.name)
    elif command == 'ios-device':
        ipa = exactly_one((APP / 'gen/apple/build').rglob('*.ipa'))
        shutil.copy2(ipa, OUT / 'mobile-companion-ios-device.ipa')
    elif command == 'checksums':
        files = sorted(p for p in OUT.iterdir() if p.is_file() and p.suffix in ['.apk', '.gz', '.ipa'])
        if not files:
            raise ValueError('No release artifacts')
        (OUT / 'SHA256SUMS').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in files))
    else:
        raise ValueError(f'Unknown command: {command}')


if __name__ == '__main__':
    main(*sys.argv[1:])
