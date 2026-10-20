"""Observable release behavior in disposable Git repositories; no network or publishing."""

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


def notes(version="0.1.1", removed=False):
    return (f"## {version} - 2026-10-20\n\n### Added\n- None.\n\n"
            "### Modified\n- None.\n\n### Fixed\n- Restore reconnect after a network interruption.\n"
            + ("\n### Removed\n- Remove the obsolete pairing command.\n" if removed else ""))


class RepositoryTest(unittest.TestCase):
    def setUp(self):
        # Keep child Git processes inside the writable checkout on Windows sandboxes.
        temp_root = Path(__file__).resolve().parents[1] / ".research/release-tests"
        temp_root.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="repo-", dir=temp_root)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Release Tests")
        self.git("config", "user.email", "release-tests@example.invalid")
        self.config = json.loads((Path(__file__).resolve().parents[1] / release.CONFIG_PATH).read_text(encoding="utf-8"))
        self.write(release.CONFIG_PATH, json.dumps(self.config, indent=2) + "\n")
        for component, data in self.config["components"].items():
            self.write(data["changelog"], f"# {data['name']} changelog\n")
            for binding in data["versionFiles"]:
                kind = binding["kind"]
                if kind == "json":
                    content = '{"name": "fixture", "version": "0.1.0", "unchanged": true}\n'
                elif kind == "c-macro":
                    content = f'#define {binding["macro"]} "2026.10.20.1"\n#define OTHER "keep"\n'
                else:
                    heading = "[[package]]" if kind == "cargo-lock" else "[package]"
                    content = f'{heading}\nname = "{binding["package"]}"\nversion = "0.1.0"\n'
                    if kind == "cargo-lock":
                        content += '\n[[package]]\nname = "unrelated-dependency"\nversion = "9.8.7"\n'
                self.write(binding["path"], content)
            self.write(data["sourcePaths"][0] + "/feature.txt", "Initial feature\n")
        self.git("add", ".")
        self.git("commit", "-m", "Initial component implementation")
        self.baseline = self.git("rev-parse", "HEAD")
        for component in release.COMPONENTS:
            self.git("tag", "-a", f"{component}-v0.1.0", "-m", "Previous release")
            data = self.config["components"][component]
            self.write(data["sourcePaths"][0] + "/feature.txt", "Reconnect correctly\n")
        self.git("add", ".")
        self.git("commit", "-m", "Fix reconnect")
        self.notes_dir = tempfile.TemporaryDirectory(prefix="notes-", dir=temp_root)
        self.addCleanup(self.notes_dir.cleanup)

    def git(self, *args):
        result = subprocess.run(["git", *args], cwd=self.root, text=True,
                                capture_output=True, encoding="utf-8", check=True)
        return result.stdout.strip()

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    def plan(self, component="cockpit", version="0.1.1"):
        path = Path(self.notes_dir.name) / f"{component}.md"
        path.write_text(notes(version), encoding="utf-8")
        return {component: {"version": version, "notesFile": str(path)}}

    def published(self, root, component):
        return {"tag_name": f"{component}-v0.1.0", "published_at": "2026-10-19T12:00:00Z"}

    def test_prepare_syncs_selected_manifest_versions_and_exports_only_new_notes(self):
        mobile_before = (self.root / "apps/companion/package.json").read_bytes()
        old_entry = notes("0.0.9")
        self.write("changelogs/cockpit.md", "# Cockpit changelog\n\n" + old_entry)
        self.git("add", ".")
        self.git("commit", "-m", "Import old release notes")
        with patch.object(release, "latest_release", side_effect=self.published):
            result = release.prepare(self.root, self.plan())
        output = self.root / ".research/release-notes.md"
        metadata = release.validate(self.root, "cockpit-v0.1.1", output)
        self.assertEqual(metadata["component"], "cockpit")
        self.assertEqual(output.read_text(encoding="utf-8"), notes())
        self.assertIn(old_entry, (self.root / "changelogs/cockpit.md").read_text(encoding="utf-8"))
        self.assertEqual((self.root / "apps/companion/package.json").read_bytes(), mobile_before)
        self.assertEqual(self.git("log", "-1", "--format=%s"), "Import old release notes")
        self.assertNotIn("cockpit-v0.1.1", self.git("tag").splitlines())
        self.assertIn('version = "9.8.7"', (self.root / "src-tauri/Cargo.lock").read_text())
        self.assertEqual(set(result["files"]), {
            release.CONFIG_PATH, "changelogs/cockpit.md", "package.json",
            "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock"})

    def test_all_prepares_independent_versions_and_migrates_firmware_only_when_preparing(self):
        plans = {}
        for component, version in zip(release.COMPONENTS, ("0.2.0", "0.1.3", "0.3.0")):
            plans.update(self.plan(component, version))
        before = {path: path.read_bytes() for path in self.root.rglob("*") if path.is_file() and ".git" not in path.parts}
        with patch.object(release, "latest_release", side_effect=self.published):
            dry_run = release.prepare(self.root, plans, dry_run=True)
        self.assertTrue(dry_run["dry_run"])
        self.assertEqual(before, {path: path.read_bytes() for path in before})
        with patch.object(release, "latest_release", side_effect=self.published):
            result = release.prepare(self.root, plans)
        self.assertEqual([item["tag"] for item in result["releases"]],
                         ["cockpit-v0.2.0", "esp32-companion-v0.1.3", "mobile-companion-v0.3.0"])
        for item in result["releases"]:
            release.validate(self.root, item["tag"])
        self.assertIn('OSG_FIRMWARE_VERSION "0.1.3"', (self.root / "apps/companion-esp32/network/firmware.h").read_text())

    def test_all_prepares_changed_components_and_atomically_skips_unchanged_mobile(self):
        mobile = self.config["components"]["mobile-companion"]
        self.write(mobile["sourcePaths"][0] + "/feature.txt", "Initial feature\n")
        self.git("add", ".")
        self.git("commit", "-m", "Restore mobile to its published content")
        untouched = [self.root / binding["path"] for binding in mobile["versionFiles"]]
        untouched.append(self.root / mobile["changelog"])
        before = {path: path.read_bytes() for path in untouched}
        plans = self.plan("cockpit", "0.2.0") | self.plan("esp32-companion", "0.1.2")
        plans["mobile-companion"] = {"skip": True, "reason": "No changes since its published release"}
        with patch.object(release, "latest_release", side_effect=self.published):
            dry_run = release.prepare(self.root, plans, dry_run=True)
            self.assertEqual(self.git("status", "--porcelain"), "")
            result = release.prepare(self.root, plans)
        self.assertEqual(dry_run["tags"], result["tags"])
        self.assertEqual(result["tags"], ["cockpit-v0.2.0", "esp32-companion-v0.1.2"])
        self.assertEqual([item["component"] for item in result["skipped"]], ["mobile-companion"])
        self.assertEqual(result["skipped"][0]["baseline"], "mobile-companion-v0.1.0")
        self.assertFalse(any("mobile-companion.md" in name or name.startswith("apps/companion/")
                             for name in result["files"]))
        self.assertEqual(before, {path: path.read_bytes() for path in untouched})
        current = json.loads((self.root / release.CONFIG_PATH).read_text())
        self.assertEqual(current["components"]["mobile-companion"]["version"], "0.1.0")
        for tag in result["tags"]:
            release.validate(self.root, tag)

    def test_skip_rejects_changed_scope_including_shared_dependencies_without_partial_edits(self):
        plans = self.plan("cockpit")
        plans["mobile-companion"] = {"skip": True, "reason": "No changes"}
        with patch.object(release, "latest_release", side_effect=self.published):
            with self.assertRaisesRegex(release.ReleaseError, "scoped diff is not empty"):
                release.prepare(self.root, plans)
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.write("apps/companion/feature.txt", "Initial feature\n")
        self.write("packages/companion-ui/shared.txt", "Shared change\n")
        self.git("add", ".")
        self.git("commit", "-m", "Change a dependency shared with mobile")
        with patch.object(release, "latest_release", side_effect=self.published):
            with self.assertRaisesRegex(release.ReleaseError, "Cannot skip mobile-companion"):
                release.prepare(self.root, plans)
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_all_unchanged_has_no_prepared_release_or_file_changes(self):
        for component in release.COMPONENTS:
            self.write(self.config["components"][component]["sourcePaths"][0] + "/feature.txt", "Initial feature\n")
        self.git("add", ".")
        self.git("commit", "-m", "Restore all published component contents")
        plans = {component: {"skip": True, "reason": "No changes"} for component in release.COMPONENTS}
        with patch.object(release, "latest_release", side_effect=self.published):
            with self.assertRaisesRegex(release.ReleaseError, "No components need a release"):
                release.prepare(self.root, plans)
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_skip_marker_requires_reason_and_does_not_accept_version_metadata(self):
        self.write("apps/companion/feature.txt", "Initial feature\n")
        self.git("add", ".")
        self.git("commit", "-m", "Restore mobile published content")
        for skip_plan in ({"skip": True}, {"skip": False, "reason": "No changes"},
                          {"skip": True, "reason": " "},
                          {"skip": True, "reason": "No changes", "version": "0.1.1"}):
            with self.subTest(plan=skip_plan), patch.object(release, "latest_release", side_effect=self.published):
                with self.assertRaises(release.ReleaseError):
                    release.prepare(self.root, self.plan("cockpit") | {"mobile-companion": skip_plan})
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_prepare_rejects_unrelated_dirty_work_without_modifying_it(self):
        self.write("unrelated.txt", "User work\n")
        with self.assertRaisesRegex(release.ReleaseError, "clean committed main"):
            release.prepare(self.root, self.plan())
        self.assertEqual((self.root / "unrelated.txt").read_text(), "User work\n")
        self.assertEqual(json.loads((self.root / "package.json").read_text())["version"], "0.1.0")

    def test_prepare_rejects_branch_and_nonincreasing_release_version(self):
        self.git("checkout", "-b", "feature")
        with self.assertRaisesRegex(release.ReleaseError, "on main"):
            release.prepare(self.root, self.plan())
        self.git("checkout", "main")
        with patch.object(release, "latest_release", side_effect=self.published):
            with self.assertRaisesRegex(release.ReleaseError, "newer than published"):
                release.prepare(self.root, self.plan(version="0.1.0"))

    def test_invalid_second_component_does_not_partly_prepare_first(self):
        plans = self.plan("cockpit") | self.plan("mobile-companion", "0.1.0")
        before = (self.root / "package.json").read_bytes()
        with patch.object(release, "latest_release", side_effect=self.published):
            with self.assertRaises(release.ReleaseError):
                release.prepare(self.root, plans)
        self.assertEqual((self.root / "package.json").read_bytes(), before)
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_inspect_includes_shared_dependencies_and_rejects_nonancestor(self):
        self.write("packages/companion-ui/shared.txt", "Shared user-visible change\n")
        self.write("apps/companion/unrelated.txt", "Mobile-only feature\n")
        self.git("add", ".")
        self.git("commit", "-m", "Update shared panel UI")
        with patch.object(release, "latest_release", side_effect=self.published):
            evidence = release.inspect_component(self.root, "cockpit")
        self.assertTrue(any("packages/companion-ui/shared.txt" in item for item in evidence["changed_files"]))
        self.assertFalse(any("apps/companion/unrelated.txt" in item for item in evidence["changed_files"]))
        self.git("checkout", "--orphan", "detached-history")
        self.git("rm", "-rf", ".")
        self.write("other.txt", "Other history\n")
        self.git("add", ".")
        self.git("commit", "-m", "Unrelated root")
        self.git("tag", "legacy-unrelated")
        self.git("checkout", "main")
        with patch.object(release, "latest_release", return_value=None):
            with self.assertRaises(release.ReleaseError):
                release.inspect_component(self.root, "cockpit", "legacy-unrelated")

    def test_first_release_includes_repository_root_or_explicit_legacy_baseline(self):
        with patch.object(release, "latest_release", return_value=None):
            root = release.inspect_component(self.root, "mobile-companion")
            explicit = release.inspect_component(self.root, "mobile-companion", self.baseline)
        self.assertEqual(root["baseline_kind"], "repository-root")
        self.assertTrue(any("apps/companion/package.json" in line for line in root["changed_files"]))
        self.assertFalse(any("apps/companion/package.json" in line for line in explicit["changed_files"]))

    def test_validate_fails_on_manifest_or_lockfile_drift(self):
        with patch.object(release, "latest_release", side_effect=self.published):
            release.prepare(self.root, self.plan())
        lock = self.root / "src-tauri/Cargo.lock"
        lock.write_text(lock.read_text().replace('version = "0.1.1"', 'version = "0.1.0"'))
        with self.assertRaisesRegex(release.ReleaseError, "Cargo.lock has version"):
            release.validate(self.root, "cockpit-v0.1.1")

    def test_validation_cli_exports_workflow_metadata_and_matching_notes(self):
        with patch.object(release, "latest_release", side_effect=self.published):
            release.prepare(self.root, self.plan())
        output = self.root / ".research/github-output.txt"
        output.parent.mkdir(parents=True, exist_ok=True)
        command = [sys.executable, str(Path(release.__file__).resolve()), "--root", str(self.root),
                   "validate", "cockpit-v0.1.1", "--notes-output", ".research/release-notes.md",
                   "--github-output", str(output)]
        result = subprocess.run(command, text=True, capture_output=True, encoding="utf-8", check=True)
        metadata = json.loads(result.stdout)
        values = dict(line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines())
        self.assertEqual(values, metadata)
        self.assertEqual(values["component"], "cockpit")
        self.assertEqual(values["version"], "0.1.1")
        self.assertEqual(values["tag"], "cockpit-v0.1.1")
        self.assertEqual(Path(values["notes_file"]).read_text(encoding="utf-8"), notes())


class ContractTest(unittest.TestCase):
    def test_latest_published_release_is_component_specific_and_uses_publish_date(self):
        releases = [[
            {"tag_name": "cockpit-v0.4.0", "published_at": "2026-10-18T12:00:00Z", "draft": False},
            {"tag_name": "mobile-companion-v9.0.0", "published_at": "2026-10-20T12:00:00Z", "draft": False},
            {"tag_name": "cockpit-v0.5.0", "published_at": None, "draft": True},
            {"tag_name": "cockpit-v0.3.9", "published_at": "2026-10-19T12:00:00Z", "draft": False},
            {"tag_name": "cockpit-build-9999", "published_at": "2026-10-20T12:00:00Z", "draft": False},
        ]]
        with patch.object(release, "run", return_value=json.dumps(releases)):
            latest = release.latest_release(Path("."), "cockpit")
        self.assertEqual(latest["tag_name"], "cockpit-v0.3.9")

    def test_tags_are_strict_and_component_specific(self):
        for component in release.COMPONENTS:
            self.assertEqual(release.parse_tag(f"{component}-v1.2.3"), (component, "1.2.3"))
        for tag in ("v1.2.3", "all-v1.2.3", "cockpit-v01.2.3", "cockpit-v1.2", "cockpit-v1.2.3-beta.1", "cockpit-v1.2.3\n", "cockpit-v1.2.3-extra"):
            with self.subTest(tag=tag), self.assertRaises(release.ReleaseError):
                release.parse_tag(tag)

    def test_bump_semver_including_pre_one_breaking_changes(self):
        self.assertEqual(release.bump_version("0.4.9", "breaking"), "0.5.0")
        self.assertEqual(release.bump_version("1.4.9", "breaking"), "2.0.0")
        self.assertEqual(release.bump_version("0.4.9", "feature"), "0.5.0")
        self.assertEqual(release.bump_version("0.4.9", "fix"), "0.4.10")

    def test_notes_allow_removed_and_reject_missing_empty_duplicate_and_malformed_entries(self):
        self.assertIn("### Removed", release.entry_text(notes(removed=True), "0.1.1"))
        malformed = [notes().replace("### Fixed", "### Fixes"), notes().replace("- Restore reconnect after a network interruption.", "- None."),
                     notes() + "\n" + notes(), notes().replace("2026-10-20", "2026-02-30"), notes() + "\n### Removed\n- None.\n"]
        for value in malformed:
            with self.subTest(value=value), self.assertRaises(release.ReleaseError):
                release.entry_text(value, "0.1.1")


if __name__ == "__main__":
    unittest.main()
