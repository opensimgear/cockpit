import base64
import hashlib
import importlib.util
import os
from pathlib import Path
import plistlib
import tarfile
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('mobile', Path(__file__).with_name('companion-mobile.py'))
mobile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mobile)


class MobileArtifacts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.app = self.root / 'app'
        self.out = self.root / 'out'
        self.patch = patch.multiple(mobile, APP=self.app, OUT=self.out)
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def artifact(self, name, data=b'artifact'):
        path = self.app / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return path

    def test_debug_apk_is_collected_without_unsigned_release(self):
        self.artifact('gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk')
        self.artifact('gen/android/app/build/outputs/apk/arm64/release/app-arm64-release-unsigned.apk', b'unsigned')
        mobile.main('android-debug')
        self.assertEqual((self.out / 'mobile-companion-android-arm64-debug.apk').read_bytes(), b'artifact')

    def test_missing_and_ambiguous_artifacts_fail(self):
        with self.assertRaises(ValueError):
            mobile.main('android-debug')
        for name in ['one-debug.apk', 'two-debug.apk']:
            self.artifact('gen/android/app/build/outputs/apk/debug/' + name)
        with self.assertRaises(ValueError):
            mobile.main('android-debug')

    def test_simulator_export_is_packaged_and_permissions_preserved(self):
        app = 'gen/apple/build/arm64-sim/Companion.app/'
        self.artifact(app + 'Info.plist', plistlib.dumps({'CFBundleSupportedPlatforms': ['iPhoneSimulator']}))
        executable = self.artifact(app + 'Companion')
        executable.chmod(0o755)
        # An intermediate/device build must not be selected instead of the exported simulator app.
        self.artifact('gen/apple/build/Companion.xcarchive/Products/Applications/Other.app/Info.plist')
        mobile.main('ios-simulator')
        with tarfile.open(self.out / 'mobile-companion-ios-arm64-simulator.app.tar.gz') as archive:
            self.assertEqual(archive.getmember('Companion.app/Companion').mode & 0o777, 0o755)

    def test_device_app_cannot_be_mislabeled_as_simulator(self):
        self.artifact('gen/apple/build/arm64-sim/Companion.app/Info.plist', plistlib.dumps({'CFBundleSupportedPlatforms': ['iPhoneOS']}))
        with self.assertRaisesRegex(ValueError, 'simulator'):
            mobile.main('ios-simulator')
        self.assertFalse(list(self.out.glob('*.tar.gz')))

    def test_ipa_collection_and_checksums(self):
        self.artifact('gen/apple/build/arm64/Companion.ipa', b'ipa')
        mobile.main('ios-device')
        mobile.main('checksums')
        self.assertEqual((self.out / 'SHA256SUMS').read_text(), hashlib.sha256(b'ipa').hexdigest() + '  mobile-companion-ios-device.ipa\n')

    def test_signing_uses_environment_passwords_and_removes_temporary_key(self):
        self.artifact('gen/android/app/build/outputs/apk/arm64/release/app-arm64-release-unsigned.apk')
        env = {'ANDROID_HOME': str(self.root / 'sdk'), 'ANDROID_KEYSTORE_BASE64': base64.b64encode(b'key').decode(),
               'ANDROID_KEYSTORE_PASSWORD': 'store-secret', 'ANDROID_KEY_PASSWORD': 'key-secret', 'ANDROID_KEY_ALIAS': 'upload'}
        calls = []
        def run(args, **kwargs):
            calls.append(args)
            if 'sign' in args:
                self.assertTrue(Path(args[args.index('--ks') + 1]).exists())
        with patch.dict(os.environ, env, clear=True), patch.object(mobile.subprocess, 'run', side_effect=run):
            mobile.main('android-sign')
        self.assertEqual(len(calls), 3)
        self.assertNotIn('store-secret', str(calls))
        self.assertNotIn('key-secret', str(calls))
        key = Path(calls[1][calls[1].index('--ks') + 1])
        self.assertFalse(key.exists())
        self.assertIn('verify', calls[2])

    def test_missing_signing_credentials_fail(self):
        with patch.dict(os.environ, {}, clear=True), self.assertRaisesRegex(ValueError, 'Missing signing secret'):
            mobile.main('android-sign')


if __name__ == '__main__':
    unittest.main()
