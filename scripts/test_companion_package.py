import importlib.util
import json
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location("companion_package", Path(__file__).with_name("companion-package.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class PackageTests(unittest.TestCase):
    def test_native_plans_are_locked_unsigned_development_only(self):
        for system, target, kind in [("windows", "x86_64-pc-windows-msvc", "nsis"), ("linux", "x86_64-unknown-linux-gnu", "deb"), ("macos", "aarch64-apple-darwin", "app")]:
            commands = package.build_commands(target, system)
            self.assertIn("--locked", commands[0])
            self.assertIn("--no-sign", commands[1])
            self.assertIn("--debug", commands[1])
            self.assertEqual(commands[1][-2:], ["--", "--locked"])
            self.assertEqual(commands[1][commands[1].index("--bundles") + 1], kind)
        with self.assertRaises(ValueError):
            package.build_commands("x86_64-pc-windows-gnu", "windows")
        with self.assertRaises(ValueError):
            package.build_commands("aarch64-linux-android", "linux")

    def test_dependency_check_follows_only_the_resolved_companion_graph(self):
        metadata = {"packages": [{"id": "root", "name": "osg-companion"}, {"id": "service", "name": "osg-companion-service"}, {"id": "host", "name": "osg-cockpit"}], "resolve": {"root": "root", "nodes": [{"id": "root", "deps": [{"pkg": "service"}]}, {"id": "service", "deps": []}, {"id": "host", "deps": []}]}}
        self.assertEqual(package.dependency_names(metadata), ["osg-companion", "osg-companion-service"])
        metadata["resolve"]["nodes"][1]["deps"].append({"pkg": "host"})
        with self.assertRaisesRegex(ValueError, "host/controller"):
            package.dependency_names(metadata)

    def test_manifest_detects_tampering_traversal_and_false_release_claims(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifact = root / "Companion-setup.exe"
            artifact.write_bytes(b"MZtest installer")
            item = package.verify_artifact(artifact, "nsis")
            evidence = {"schemaVersion": 1, "buildKind": "development", "signingRequested": False, "published": False, "installed": False, "files": [item]}
            manifest = root / "development-evidence.json"
            def save():
                manifest.write_text(json.dumps(evidence), encoding="utf-8")
            save()
            self.assertEqual(package.verify_manifest(manifest)["files"], [item])
            artifact.write_bytes(b"MZchanged installer")
            with self.assertRaisesRegex(ValueError, "match"):
                package.verify_manifest(manifest)
            evidence["files"][0]["path"] = "../Companion-setup.exe"
            save()
            with self.assertRaisesRegex(ValueError, "escapes"):
                package.verify_manifest(manifest)
            evidence["published"] = True
            save()
            with self.assertRaisesRegex(ValueError, "development"):
                package.verify_manifest(manifest)

    def test_native_formats_require_the_companion_and_do_not_execute_installers(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            invalid = root / "invalid.exe"
            invalid.write_bytes(b"no native executable")
            with self.assertRaises(ValueError):
                package.verify_artifact(invalid, "nsis")
            deb = root / "companion.deb"
            deb.write_bytes(b"archive fixture")
            with patch.object(package, "run", side_effect=["Package: open-sim-gear-companion", "./usr/bin/osg-companion"]):
                self.assertEqual(package.verify_artifact(deb, "deb")["format"], "deb")
            with patch.object(package, "run", side_effect=["Package: open-sim-gear-companion", "./usr/bin/osg-cockpit"]):
                with self.assertRaises(ValueError):
                    package.verify_artifact(deb, "deb")
            app = root / "companion.app.zip"
            with zipfile.ZipFile(app, "w") as archive:
                archive.writestr("Companion.app/Contents/Info.plist", plistlib.dumps({"CFBundleIdentifier": "org.opensimgear.companion"}))
                archive.writestr("Companion.app/Contents/MacOS/osg-companion", b"native")
            self.assertEqual(package.verify_artifact(app, "app-zip")["format"], "app-zip")
            with zipfile.ZipFile(app, "a") as archive:
                archive.writestr("../outside", b"invalid")
            with self.assertRaisesRegex(ValueError, "escaping"):
                package.verify_artifact(app, "app-zip")


if __name__ == "__main__":
    unittest.main()
