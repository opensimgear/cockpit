import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("publisher", Path(__file__).with_name("publish-component-release.py"))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)
SOURCE = "a" * 40


def assets(root, payload=b"build", source=SOURCE):
    root.mkdir(parents=True, exist_ok=True)
    artifact = root / "cockpit-1.2.3-linux-x64.deb"
    artifact.write_bytes(payload)
    metadata = dict(schemaVersion=1, component="cockpit", version="1.2.3", sourceCommit=source,
                    assets=[dict(name=artifact.name, size=len(payload), sha256=hashlib.sha256(payload).hexdigest())])
    (root / "release-manifest.json").write_text(json.dumps(metadata) + "\n", encoding="utf-8")
    (root / "SHA256SUMS").write_text("".join(
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
        for path in sorted(root.iterdir()) if path.name != "SHA256SUMS"), encoding="ascii")
    return root


class FakeGitHub:
    def __init__(self, root, notes):
        self.root = root
        self.root.mkdir()
        self.notes = notes
        self.exists = False
        self.draft = True
        self.calls = []

    def run(self, command, **kwargs):
        if command == ["git", "rev-parse", "HEAD"]:
            return subprocess.CompletedProcess(command, 0, SOURCE + "\n", "")
        self.calls.append(command)
        args = command[1:]
        action = args[:2]
        if action == ["release", "view"]:
            if not self.exists:
                return subprocess.CompletedProcess(command, 1, "", "release not found")
            metadata = dict(isDraft=self.draft, body=self.notes, url="https://example.com/release",
                            name="cockpit 1.2.3", assets=[dict(name=path.name) for path in self.root.iterdir()])
            return subprocess.CompletedProcess(command, 0, json.dumps(metadata), "")
        if action == ["release", "create"]:
            self.exists = True
            self.notes = Path(args[args.index("--notes-file") + 1]).read_text(encoding="utf-8")
        elif action == ["release", "edit"]:
            if "--notes-file" in args:
                self.notes = Path(args[args.index("--notes-file") + 1]).read_text(encoding="utf-8")
            if "--draft=false" in args:
                self.draft = False
        elif action == ["release", "upload"]:
            if not self.draft:
                raise AssertionError("Published releases must never receive uploads")
            for name in args[3:]:
                if name != "--clobber":
                    shutil.copy2(name, self.root / Path(name).name)
        elif action == ["release", "download"]:
            destination = Path(args[args.index("--dir") + 1])
            for path in self.root.iterdir():
                shutil.copy2(path, destination / path.name)
        else:
            raise AssertionError("Unexpected GitHub command: " + repr(command))
        return subprocess.CompletedProcess(command, 0, "", "")


class ComponentPublication(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.local = assets(self.root / "local")
        self.notes = self.root / "notes.md"
        self.notes.write_text("## 1.2.3\n\nRelease changes.\n", encoding="utf-8")
        self.github = FakeGitHub(self.root / "github", self.notes.read_text(encoding="utf-8"))
        self.environment = patch.dict(os.environ, {"GITHUB_SHA": SOURCE})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.commands = patch.object(publisher.subprocess, "run", side_effect=self.github.run)
        self.commands.start()
        self.addCleanup(self.commands.stop)

    def publish(self):
        return publisher.publish("cockpit-v1.2.3", "cockpit", "1.2.3", self.notes, self.local)

    def seed_published(self):
        self.github.exists = True
        self.github.draft = False
        for path in self.local.iterdir():
            shutil.copy2(path, self.github.root / path.name)

    def test_new_release_stays_draft_until_verified_assets_are_uploaded(self):
        result = self.publish()
        self.assertEqual(result["published"], "true")
        self.assertFalse(self.github.draft)
        mutations = [command[2] for command in self.github.calls if command[2] in ("create", "upload", "download", "edit")]
        self.assertEqual(mutations, ["create", "upload", "download", "edit"])
        self.assertIn("--prerelease=false", self.github.calls[-2])

    def test_published_rerun_preserves_original_bytes_after_nondeterministic_rebuild(self):
        self.seed_published()
        original = (self.github.root / "cockpit-1.2.3-linux-x64.deb").read_bytes()
        assets(self.local, payload=b"different rebuild bytes")
        result = self.publish()
        self.assertEqual(result["published"], "false")
        self.assertEqual((self.github.root / "cockpit-1.2.3-linux-x64.deb").read_bytes(), original)
        self.assertFalse(any(command[2] in ("create", "edit", "upload") for command in self.github.calls))

    def test_published_release_with_wrong_source_is_rejected_without_mutation(self):
        self.seed_published()
        assets(self.github.root, source="b" * 40)
        with self.assertRaisesRegex(ValueError, "source commit"):
            self.publish()
        self.assertFalse(any(command[2] in ("create", "edit", "upload") for command in self.github.calls))

    def test_published_release_with_missing_asset_is_not_repaired_or_overwritten(self):
        self.seed_published()
        (self.github.root / "cockpit-1.2.3-linux-x64.deb").unlink()
        with self.assertRaisesRegex(ValueError, "complete expected"):
            self.publish()
        self.assertFalse(any(command[2] in ("create", "edit", "upload") for command in self.github.calls))

    def test_notes_change_is_rejected_for_published_release(self):
        self.seed_published()
        self.github.notes = "Unexpected edited notes"
        with self.assertRaisesRegex(ValueError, "committed release notes"):
            self.publish()

    def test_interrupted_draft_can_replace_partial_draft_payload(self):
        self.github.exists = True
        (self.github.root / "cockpit-1.2.3-linux-x64.deb").write_bytes(b"partial stale bytes")
        self.assertEqual(self.publish()["published"], "true")
        self.assertEqual((self.github.root / "cockpit-1.2.3-linux-x64.deb").read_bytes(), b"build")

    def test_local_corruption_is_rejected_before_any_github_request(self):
        (self.local / "cockpit-1.2.3-linux-x64.deb").write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            self.publish()
        self.assertEqual(self.github.calls, [])

    def test_unknown_draft_asset_is_preserved_and_blocks_publication(self):
        self.github.exists = True
        (self.github.root / "unrelated.txt").write_text("Keep me", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "unexpected assets"):
            self.publish()
        self.assertTrue(self.github.draft)

    def test_lookup_network_error_never_attempts_release_creation(self):
        with patch.object(publisher, "gh", return_value=subprocess.CompletedProcess([], 1, "", "network unavailable")):
            with self.assertRaisesRegex(RuntimeError, "Cannot inspect"):
                self.publish()
        self.assertEqual(self.github.calls, [])


if __name__ == "__main__":
    unittest.main()
