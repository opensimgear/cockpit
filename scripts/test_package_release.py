import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location("package_release", Path(__file__).with_name("package-release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleasePackages(unittest.TestCase):
    def setUp(self):
        temporary_root = Path(__file__).resolve().parents[1] / ".research/release-package-tests"
        temporary_root.mkdir(parents=True, exist_ok=True)
        directory = tempfile.TemporaryDirectory(dir=temporary_root)
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.source = self.root / "source"
        self.source.mkdir()
        self.output = self.root / "output"
        self.version = "0.2.1"
        self.commit = "a" * 40
        self.patcher = patch.object(release, "source_commit", return_value=self.commit)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)

    def board_source(self, board):
        directory = self.source / board
        directory.mkdir()
        chip, boot = release.BOARDS[board]
        records = []
        for name, offset in zip(release.SEGMENTS, (boot, "0x8000", "0x10000")):
            image = bytearray(32)
            image[0] = 0xE9
            image[12] = 9 if chip == "esp32s3" else 0
            content = b"partition" if name == "partition-table.bin" else bytes(image)
            (directory / name).write_bytes(content)
            records.append({"offset": offset, "file": name, "sha256": hashlib.sha256(content).hexdigest()})
        (directory / "manifest.json").write_text(json.dumps({"board": board, "version": self.version, "sourceCommit": self.commit, "chip": chip, "segments": records}))
        return directory

    def test_firmware_release_contains_both_verified_boards_and_aggregate_checksums(self):
        staged = self.root / "staged"
        for board in release.BOARDS:
            source = self.board_source(board)
            archive = release.firmware(board, self.version, source, staged)
            with zipfile.ZipFile(archive) as package:
                self.assertEqual(set(package.namelist()), {"manifest.json", "README.md", *release.SEGMENTS})
                manifest = json.loads(package.read("manifest.json"))
                self.assertEqual(manifest["version"], self.version)
                self.assertEqual(manifest["board"], board)
        release.finalize("esp32-companion", self.version, staged, self.output)
        metadata = json.loads((self.output / "release-manifest.json").read_text())
        self.assertEqual(metadata["sourceCommit"], self.commit)
        self.assertEqual(len(metadata["assets"]), 2)
        lines = (self.output / "SHA256SUMS").read_text().splitlines()
        self.assertEqual(len(lines), 3)
        for line in lines:
            checksum, name = line.split("  ")
            self.assertEqual(checksum, release.digest(self.output / name))

    def test_firmware_rejects_wrong_board_version_source_and_corrupt_bytes(self):
        board = "waveshare-lcd7-s3"
        source = self.board_source(board)
        path = source / "manifest.json"
        original = json.loads(path.read_text())
        for key, value in (("board", "sunton-3248s035r"), ("version", "9.9.9"), ("sourceCommit", "b" * 40)):
            with self.subTest(key=key):
                path.write_text(json.dumps({**original, key: value}))
                with self.assertRaises(ValueError):
                    release.firmware_manifest(board, self.version, source)
        path.write_text(json.dumps(original))
        (source / "application.bin").write_bytes(b"\xe9corrupt")
        with self.assertRaises(ValueError):
            release.firmware_manifest(board, self.version, source)

    def test_partial_component_cannot_be_published(self):
        (self.source / f"cockpit-{self.version}-windows-x64-setup.exe").write_bytes(b"installer")
        with self.assertRaisesRegex(ValueError, "Incomplete"):
            release.finalize("cockpit", self.version, self.source, self.output)

    def test_chip_headers_and_partition_sizes_match_runtime_requirements(self):
        content = {name: bytes([0xE9]) + bytes(31) for name in release.SEGMENTS}
        with self.assertRaisesRegex(ValueError, "wrong chip"):
            release.validate_segments("waveshare-lcd7-s3", content)
        release.validate_segments("sunton-3248s035r", content)
        content["partition-table.bin"] = bytes(0x8001)
        with self.assertRaisesRegex(ValueError, "partition"):
            release.validate_segments("sunton-3248s035r", content)

    def test_mobile_requires_android_and_simulator_and_keeps_debug_label(self):
        (self.source / "mobile-companion-android-arm64-debug.apk").write_bytes(b"debug-apk")
        staged = self.root / "staged"
        release.mobile(self.version, self.source, staged)
        with self.assertRaisesRegex(ValueError, "Incomplete"):
            release.finalize("mobile-companion", self.version, staged, self.output)
        (self.source / "mobile-companion-ios-arm64-simulator.app.tar.gz").write_bytes(b"simulator")
        release.mobile(self.version, self.source, staged)
        release.finalize("mobile-companion", self.version, staged, self.output)
        names = {entry["name"] for entry in json.loads((self.output / "release-manifest.json").read_text())["assets"]}
        self.assertIn(f"mobile-companion-{self.version}-android-arm64-debug.apk", names)

    def test_no_cross_component_or_duplicate_assets(self):
        name = f"mobile-companion-{self.version}-ios-arm64-simulator.app.tar.gz"
        (self.source / name).write_bytes(b"simulator")
        (self.source / f"mobile-companion-{self.version}-android-arm64-debug.apk").write_bytes(b"apk")
        (self.source / "unrelated.zip").write_bytes(b"unexpected")
        with self.assertRaisesRegex(ValueError, "unexpected"):
            release.finalize("mobile-companion", self.version, self.source, self.output)
        (self.source / "nested").mkdir()
        (self.source / "nested" / name).write_bytes(b"duplicate")
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            release.finalize("mobile-companion", self.version, self.source, self.output)

    def test_rejects_invalid_version_and_empty_assets(self):
        for value in ("01.2.3", "1.2", "../1.2.3", "1.2.3\nanything"):
            with self.assertRaises(ValueError):
                release.version_value(value)
        empty = self.source / "empty.apk"
        empty.touch()
        with self.assertRaisesRegex(ValueError, "Empty"):
            release.copy_asset(empty, self.output, empty.name)


if __name__ == "__main__":
    unittest.main()
