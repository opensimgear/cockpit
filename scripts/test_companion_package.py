import importlib.util
import json
import copy
import io
import struct
import sys
import stat
import warnings
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
                archive.writestr("Companion.app/Contents/MacOS/osg-companion", b"native" * 4)
            self.assertEqual(package.verify_artifact(app, "app-zip")["format"], "app-zip")
            with zipfile.ZipFile(app, "a") as archive:
                archive.writestr("../outside", b"invalid")
            with self.assertRaisesRegex(ValueError, "escaping"):
                package.verify_artifact(app, "app-zip")


class ContractTests(unittest.TestCase):
    identity = {"identifier": "org.opensimgear.companion", "productName": "OpenSimGear Companion", "version": "0.1.0"}

    def pe(self, path):
        header = bytearray(64)
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 60, 64)
        path.write_bytes(header + b"PE\0\0" + struct.pack("<H", 0x14c) + bytes(18))

    def app_zip(self, path, version="0.1.0", cpu=0x0100000c, **extra):
        plist = {"CFBundleIdentifier": self.identity["identifier"], "CFBundleShortVersionString": version,
                 "CFBundleExecutable": "osg-companion", **extra}
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("Companion.app/Contents/Info.plist", plistlib.dumps(plist))
            archive.writestr("Companion.app/Contents/MacOS/osg-companion", b"\xcf\xfa\xed\xfe" + struct.pack("<I", cpu) + bytes(24))

    def evidence(self, root):
        artifact = root / "OpenSimGear Companion_0.1.0_x64-setup.exe"
        self.pe(artifact)
        contract = package.compatibility_contract()
        return {"schemaVersion": 2, "buildKind": "development", "signingRequested": False, "published": False,
                "installed": False, "releaseQualified": False, "signatureVerification": "not-performed", "platform": "windows", "target": "x86_64-pc-windows-msvc",
                "application": dict(self.identity), "source": {"commit": None, "buildInputsSha256": "a" * 64, "inputCount": 5},
                "compatibility": contract, "configSha256": "b" * 64, "compatibilitySha256": package.digest_value(contract),
                "builtAtUtc": "2026-10-16T00:00:00+00:00", "commands": package.build_commands("x86_64-pc-windows-msvc", "windows"),
                "dependencies": ["osg-companion", "osg-companion-service"],
                "files": [package.verify_artifact(artifact, "nsis", self.identity, "x86_64-pc-windows-msvc")]}

    def test_current_source_contract_and_identity_are_explicit_without_credentials(self):
        config = json.loads((package.APP / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
        self.assertEqual(package.package_identity(config), self.identity)
        package.validate_versions(self.identity)
        with self.assertRaisesRegex(ValueError, "versions must agree"):
            package.validate_versions({**self.identity, "version": "0.2.0"})
        contract = package.compatibility_contract()
        self.assertEqual(contract["websocketSubprotocol"], "opensimgear-companion-v1")
        self.assertEqual(contract["contractSourcesSha256"], package.digest_value(contract["contractSources"]))
        self.assertNotIn("token", json.dumps(contract))
        for key, value in [("version", "invalid"), ("identifier", "other"), ("productName", "Other")]:
            bad = copy.deepcopy(config)
            bad[key] = value
            with self.assertRaises(ValueError):
                package.package_identity(bad)
        config["bundle"]["createUpdaterArtifacts"] = True
        with self.assertRaisesRegex(ValueError, "updater"):
            package.package_identity(config)

    def test_source_hash_is_stable_changes_with_inputs_and_ignores_generated_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("pnpm-lock.yaml", "pnpm-workspace.yaml", "package.json", "rust-toolchain.toml", "LICENSE", "apps/companion/src/main.ts"):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name, encoding="utf-8")
            first = package.source_provenance(root)
            self.assertIsNone(first["commit"])
            generated = root / "apps/companion/dist/generated.js"
            generated.parent.mkdir()
            generated.write_text("not a source", encoding="utf-8")
            self.assertEqual(package.source_provenance(root), first)
            (root / "apps/companion/src/main.ts").write_text("changed", encoding="utf-8")
            self.assertNotEqual(package.source_provenance(root)["buildInputsSha256"], first["buildInputsSha256"])
            (root / ".git").mkdir()
            with patch.object(package, "run", return_value="c" * 40):
                self.assertEqual(package.source_provenance(root)["commit"], "c" * 40)
            with patch.object(package, "run", return_value="not a commit"):
                with self.assertRaisesRegex(ValueError, "revision"):
                    package.source_provenance(root)

    def test_schema2_roundtrip_and_false_provenance_or_release_claims_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence = self.evidence(root)
            path = root / "development-evidence.json"
            path.write_text(json.dumps(evidence), encoding="utf-8")
            self.assertEqual(package.verify_manifest(path), evidence)
            for mutate in [lambda x: x.update(releaseQualified=True), lambda x: x.update(extra="unknown"),
                           lambda x: x.update(target="x86_64-unknown-linux-gnu"),
                           lambda x: x["commands"][1].remove("--no-sign"),
                           lambda x: x["dependencies"].append("osg-mapping-host"),
                           lambda x: x["source"].update(buildInputsSha256="bad"),
                           lambda x: x.update(configSha256="bad"),
                           lambda x: x["compatibility"].update(wireVersion=True),
                           lambda x: x["compatibility"].update(trustModel="trust-any"),
                           lambda x: x["compatibility"]["contractSources"].update({package.CONTRACT_FILES[0]: "d" * 64}),
                           lambda x: x.update(builtAtUtc="2026-10-16T00:00:00"),
                           lambda x: x["files"][0].update(format="deb"),
                           lambda x: x["files"].append(x["files"][0])]:
                bad = copy.deepcopy(evidence)
                mutate(bad)
                path.write_text(json.dumps(bad), encoding="utf-8")
                with self.assertRaises(ValueError):
                    package.verify_manifest(path)

    def test_windows_pe_stub_not_confused_with_payload_architecture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "Companion_0.1.0_x64-setup.exe"
            self.pe(path)
            item = package.verify_artifact(path, "nsis", self.identity, "x86_64-pc-windows-msvc")
            self.assertEqual(item["metadata"]["verification"], "pe-and-filename-only")
            for name in ["Companion_0.2.0_x64-setup.exe", "Companion_0.1.0_arm64-setup.exe"]:
                wrong = root / name
                self.pe(wrong)
                with self.assertRaisesRegex(ValueError, "filename"):
                    package.verify_artifact(wrong, "nsis", self.identity, "x86_64-pc-windows-msvc")
            path.write_bytes(b"MZshort")
            with self.assertRaisesRegex(ValueError, "PE header"):
                package.verify_artifact(path, "nsis", self.identity, "x86_64-pc-windows-msvc")
            self.pe(path)
            data = bytearray(path.read_bytes())
            struct.pack_into("<I", data, 60, 0xffffffff)
            path.write_bytes(data)
            with self.assertRaisesRegex(ValueError, "offset"):
                package.verify_artifact(path, "nsis", self.identity, "x86_64-pc-windows-msvc")
            self.pe(path)
            path.write_bytes(path.read_bytes()[:64] + b"bad" + bytes(21))
            with self.assertRaisesRegex(ValueError, "machine"):
                package.verify_artifact(path, "nsis", self.identity, "x86_64-pc-windows-msvc")

    def test_debian_version_and_architecture_match_exact_build(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "companion.deb"
            path.write_bytes(b"fixture")
            for version, architecture, okay in [("0.1.0", "amd64", True), ("0.2.0", "amd64", False), ("0.1.0", "arm64", False)]:
                fields = f"Package: open-sim-gear-companion\nVersion: {version}\nArchitecture: {architecture}"
                with patch.object(package, "run", side_effect=[fields, "./usr/bin/osg-companion"]):
                    if okay:
                        item = package.verify_artifact(path, "deb", self.identity, "x86_64-unknown-linux-gnu")
                        self.assertEqual(item["metadata"]["architecture"], "amd64")
                    else:
                        with self.assertRaisesRegex(ValueError, "version/architecture"):
                            package.verify_artifact(path, "deb", self.identity, "x86_64-unknown-linux-gnu")

    def test_macos_native_identity_architecture_and_archive_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "companion.app.zip"
            self.app_zip(path)
            item = package.verify_artifact(path, "app-zip", self.identity, "aarch64-apple-darwin")
            self.assertEqual(item["metadata"]["architectures"], ["aarch64"])
            for options in [{"version": "0.2.0"}, {"cpu": 0x01000007}, {"CFBundleExecutable": "other"}, {"CFBundleIdentifier": "other"}]:
                self.app_zip(path, **options)
                with self.assertRaises(ValueError):
                    package.verify_artifact(path, "app-zip", self.identity, "aarch64-apple-darwin")
            for name in ["../outside", "C:/outside", "a\\outside", "/absolute", "./outside"]:
                self.app_zip(path)
                with zipfile.ZipFile(path, "a") as archive:
                    archive.writestr(name.replace("\\", "/"), b"invalid")
                if "\\" in name:
                    path.write_bytes(path.read_bytes().replace(b"a/outside", b"a\\outside"))
                with self.subTest(path=name), self.assertRaisesRegex(ValueError, "escaping"):
                    package.verify_artifact(path, "app-zip", self.identity, "aarch64-apple-darwin")
            self.app_zip(path)
            with zipfile.ZipFile(path, "a") as archive:
                link = zipfile.ZipInfo("Companion.app/linked")
                link.external_attr = (stat.S_IFLNK | 0o777) << 16
                archive.writestr(link, "../../outside")
            with self.assertRaisesRegex(ValueError, "symlinks"):
                package.verify_artifact(path, "app-zip")
            self.app_zip(path)
            with warnings.catch_warnings(), zipfile.ZipFile(path, "a") as archive:
                warnings.simplefilter("ignore", UserWarning)
                archive.writestr("Companion.app/Contents/Info.plist", b"duplicate")
            with self.assertRaisesRegex(ValueError, "duplicate"):
                package.verify_artifact(path, "app-zip")

    def test_macho_thin_universal_and_malformed_headers(self):
        self.assertEqual(package.macho_architecture(b"\xfe\xed\xfa\xcf" + struct.pack(">I", 0x01000007) + bytes(8)), ["x86_64"])
        fat = b"\xca\xfe\xba\xbe" + struct.pack(">I", 2)
        fat += struct.pack(">I", 0x01000007) + bytes(16) + struct.pack(">I", 0x0100000c) + bytes(16)
        self.assertEqual(package.macho_architecture(fat), ["aarch64", "x86_64"])
        for header in [b"not a Mach-O", fat[:16], b"\xcf\xfa\xed\xfe" + bytes(12), fat[:4] + struct.pack(">I", 99)]:
            with self.assertRaisesRegex(ValueError, "Mach-O"):
                package.macho_architecture(header)

    def test_comparison_is_metadata_only_and_never_claims_trust_or_update_qualification(self):
        with tempfile.TemporaryDirectory() as directory:
            before = self.evidence(Path(directory))
            after = copy.deepcopy(before)
            after["application"]["version"] = "0.2.0"
            after["source"]["buildInputsSha256"] = "d" * 64
            result = package.compare_evidence(before, after)
            self.assertEqual(result["classification"], "metadata-compatible")
            for key in ["nativeInstallUpdateQualified", "releaseQualified", "trustMigrationVerified", "automaticUpdateEnabled"]:
                self.assertIs(result[key], False)
            after["application"]["version"] = "0.0.9"
            self.assertEqual(package.compare_evidence(before, after)["classification"], "review-required")
            after = copy.deepcopy(before)
            after["compatibility"]["contractSources"][package.CONTRACT_FILES[0]] = "d" * 64
            after["compatibility"]["contractSourcesSha256"] = package.digest_value(after["compatibility"]["contractSources"])
            after["compatibilitySha256"] = package.digest_value(after["compatibility"])
            self.assertIn("migration", package.compare_evidence(before, after)["reasons"][0])
            after = copy.deepcopy(before)
            after["target"] = "aarch64-pc-windows-msvc"
            after["commands"] = package.build_commands(after["target"], "windows")
            self.assertIn("target changed", package.compare_evidence(before, after)["reasons"])
            with self.assertRaisesRegex(ValueError, "Legacy"):
                package.compare_evidence({"schemaVersion": 1}, before)

    def test_cli_verify_compare_and_native_architecture_guard(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence = self.evidence(root)
            path = root / "development-evidence.json"
            path.write_text(json.dumps(evidence), encoding="utf-8")
            with patch.object(sys, "argv", ["package", "--verify-only", str(path), "--compare-to", str(path)]), patch("sys.stdout", new_callable=io.StringIO) as output:
                package.main()
                self.assertEqual(json.loads(output.getvalue())["scope"], "metadata-only")
            with patch.object(sys, "argv", ["package", "--verify-only", str(path)]), patch("sys.stdout", new_callable=io.StringIO) as output:
                package.main()
                self.assertIn("no signature", output.getvalue())
            with patch.object(sys, "argv", ["package", "--compare-to", str(path)]):
                with self.assertRaisesRegex(ValueError, "requires"):
                    package.main()
            with patch.object(sys, "argv", ["package", "--target", "aarch64-pc-windows-msvc"]), patch.object(package, "native_platform", return_value="windows"), patch.object(package, "run", return_value="host: x86_64-pc-windows-msvc"):
                with self.assertRaisesRegex(ValueError, "exact host"):
                    package.main()


    def test_packaging_orchestration_all_native_formats_and_stable_source(self):
        metadata = {"packages": [{"id": "app", "name": "osg-companion"}, {"id": "service", "name": "osg-companion-service"}],
                    "resolve": {"root": "app", "nodes": [{"id": "app", "deps": [{"pkg": "service"}]}, {"id": "service", "deps": []}]}}
        provenance = {"commit": None, "buildInputsSha256": "a" * 64, "inputCount": 5}
        for system, target, kind in [("windows", "x86_64-pc-windows-msvc", "nsis"), ("linux", "x86_64-unknown-linux-gnu", "deb"), ("macos", "aarch64-apple-darwin", "macos")]:
            with self.subTest(system=system), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                app = root / "app"
                (app / "src-tauri").mkdir(parents=True)
                config = {**self.identity, "bundle": {"createUpdaterArtifacts": False}}
                (app / "src-tauri/tauri.conf.json").write_text(json.dumps(config), encoding="utf-8")
                target_dir = root / "target"
                bundle = target_dir / target / "debug/bundle" / kind
                bundle.mkdir(parents=True)
                name = {"windows": "Companion_0.1.0_x64-setup.exe", "linux": "Companion_0.1.0_amd64.deb", "macos": "OpenSimGear Companion.app"}[system]
                source = bundle / name
                if system == "windows":
                    self.pe(source)
                elif system == "macos":
                    source.mkdir()
                else:
                    source.write_bytes(b"deb fixture")
                called = []
                def run(arguments, **kwargs):
                    called.append(arguments)
                    if arguments[0] == "rustc":
                        return "host: " + target
                    if arguments[:2] == ["cargo", "metadata"]:
                        return json.dumps(metadata)
                    if arguments[0] == "ditto":
                        self.app_zip(Path(arguments[-1]))
                    if arguments[:2] == ["dpkg-deb", "--field"]:
                        return "Package: open-sim-gear-companion\nVersion: 0.1.0\nArchitecture: amd64"
                    if arguments[:2] == ["dpkg-deb", "--contents"]:
                        return "./usr/bin/osg-companion"
                argv = ["package", "--output-dir", str(root / "output")]
                with patch.object(sys, "argv", argv), patch.object(package, "native_platform", return_value=system), patch.object(package, "APP", app), patch.object(package, "run", side_effect=run), patch.object(package, "source_provenance", return_value=provenance), patch.dict("os.environ", {"CARGO_TARGET_DIR": str(target_dir)}), patch("sys.stdout", new_callable=io.StringIO):
                    package.main()
                    path = root / "output" / system / "development-evidence.json"
                    result = package.verify_manifest(path)
                    self.assertEqual(result["source"], provenance)
                    self.assertEqual(result["application"], self.identity)
                    self.assertIs(result["releaseQualified"], False)
                    self.assertEqual([command for command in called if command[:2] == ["cargo", "test"]], [package.build_commands(target, system)[0]])
                    with patch.object(package, "source_provenance", side_effect=[provenance, {**provenance, "buildInputsSha256": "f" * 64}]):
                        with self.assertRaisesRegex(ValueError, "changed during packaging"):
                            package.main()
                    if source.is_file():
                        source.unlink()
                    else:
                        source.rmdir()
                    with self.assertRaisesRegex(ValueError, "exactly one"):
                        package.main()

    def test_process_runner_and_plan_do_not_install_or_publish(self):
        from types import SimpleNamespace
        with patch.object(package.shutil, "which", return_value="resolved-python"), patch.object(package.subprocess, "run", return_value=SimpleNamespace(stdout=" evidence \n")) as command:
            self.assertEqual(package.run(["python", "--version"], capture=True), "evidence")
            self.assertTrue(command.call_args.kwargs["check"])
            self.assertEqual(command.call_args.args[0], ["resolved-python", "--version"])
        with patch.object(sys, "argv", ["package", "--print-plan"]), patch.object(package, "native_platform", return_value="windows"), patch.object(package, "run", return_value="host: x86_64-pc-windows-msvc"), patch("sys.stdout", new_callable=io.StringIO) as output:
            package.main()
            self.assertIn("--no-sign", output.getvalue())
        with patch.object(sys, "argv", ["package"]), patch.object(package, "native_platform", return_value=None):
            with self.assertRaisesRegex(ValueError, "native Windows"):
                package.main()


if __name__ == "__main__":
    unittest.main()
