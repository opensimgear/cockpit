import contextlib
from copy import deepcopy
import importlib.util
import io
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("companion_release", Path(__file__).with_name("companion-release-check.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseReadinessTests(unittest.TestCase):
    def setUp(self):
        self.matrix_path = release.ROOT / "docs/companion-release-matrix.json"
        self.matrix = release.load_matrix(self.matrix_path)

    def test_real_repository_is_valid_but_no_client_is_release_qualified(self):
        result = release.validate(self.matrix)
        self.assertFalse(result["releaseEligible"])
        self.assertEqual(result["qualifiedClientTargets"], [])
        self.assertEqual(set(result["verifiedDisabledSourceGates"]), set(release.GATES))
        self.assertIn(68, result["pendingQualificationIssues"])
        self.assertEqual(result["budgets"]["releaseLatencyTargetP99Ms"], 50)

    def test_false_release_and_missing_followups_are_rejected(self):
        for field, value in [("releaseQualified", True), ("releaseStatus", "ready"),
                             ("schemaVersion", 2), ("featureIssue", 44),
                             ("pendingQualificationIssues", []), ("performanceEvidence", "measured")]:
            with self.subTest(field=field):
                changed = deepcopy(self.matrix)
                changed[field] = value
                with self.assertRaises(ValueError):
                    release.validate(changed)
        changed = deepcopy(self.matrix)
        changed["unreviewed"] = True
        with self.assertRaises(ValueError):
            release.validate(changed)
        with self.assertRaises(ValueError):
            release.validate([])

    def test_every_client_retains_platform_and_authentication_qualification(self):
        for name in self.matrix["clients"]:
            for field, value in [("status", "supported"), ("artifact", "signed-release"),
                                 ("authentication", "unrestricted"), ("qualificationIssues", []),
                                 ("candidateFloor", ""), ("candidateFloor", "bad\nfloor")]:
                with self.subTest(client=name, field=field):
                    changed = deepcopy(self.matrix)
                    changed["clients"][name][field] = value
                    with self.assertRaises(ValueError):
                        release.validate(changed)
        changed = deepcopy(self.matrix)
        del changed["clients"]["macos"]
        with self.assertRaises(ValueError):
            release.validate(changed)

    def test_documented_limits_and_all_disabled_capabilities_are_required(self):
        for name in self.matrix["budgets"]:
            changed = deepcopy(self.matrix)
            changed["budgets"][name] += 1
            with self.assertRaisesRegex(ValueError, "Budgets"):
                release.validate(changed)
        for name in release.GATES:
            changed = deepcopy(self.matrix)
            changed["disabledCapabilities"][name] = True
            with self.assertRaisesRegex(ValueError, "disabled"):
                release.validate(changed)

    def test_native_source_gate_enablement_cannot_be_hidden_by_a_false_ledger(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative, _ in release.GATES.values():
                target = root / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(release.ROOT / relative, target)
            self.assertFalse(release.validate(self.matrix, root)["releaseEligible"])
            for name, (relative, _) in release.GATES.items():
                target = root / relative
                original = target.read_text(encoding="utf-8")
                if name == "mobile-authentication":
                    changed = original.replace("if mobile {\n        Some(", "if mobile {\n        None.or(", 1)
                elif name == "host-live-output":
                    changed = original.replace("osg_virtual_controller::entry::factory(),\n            false,", "osg_virtual_controller::entry::factory(),\n            true,", 1)
                elif name == "live-scripts":
                    changed = original.replace("SCRIPT_LIVE_QUALIFIED: bool = false", "SCRIPT_LIVE_QUALIFIED: bool = true", 1)
                else:
                    changed = original.replace("QUALIFIED: bool = false", "QUALIFIED: bool = true", 1)
                self.assertNotEqual(changed, original, name)
                target.write_text(changed, encoding="utf-8")
                with self.assertRaisesRegex(ValueError, name):
                    release.validate(self.matrix, root)
                target.write_text(original, encoding="utf-8")

    def test_matrix_reader_rejects_duplicates_oversize_invalid_and_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "matrix.json"
            for raw in ['{"schemaVersion":1,"schemaVersion":1}', '{', ' ' * 65537]:
                path.write_text(raw, encoding="utf-8")
                with self.assertRaises(ValueError):
                    release.load_matrix(path)
            path.write_text('{}', encoding="utf-8")
            with patch.object(Path, "is_symlink", return_value=True):
                with self.assertRaisesRegex(ValueError, "regular"):
                    release.load_matrix(path)

    def test_cli_writes_truthful_report_and_release_requirement_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "nested/readiness.json"
            for extra, expected in [([], 0), (["--require-release"], 2)]:
                stdout = io.StringIO()
                with contextlib.redirect_stdout(stdout):
                    result = release.main(["--output", str(output), *extra])
                self.assertEqual(result, expected)
                self.assertFalse(json.loads(stdout.getvalue())["releaseEligible"])
                self.assertEqual(json.loads(output.read_text()), json.loads(stdout.getvalue()))
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = release.main(["--matrix", str(Path(directory) / "missing")])
            self.assertEqual(result, 1)
            self.assertTrue(stderr.getvalue())


if __name__ == "__main__":
    unittest.main()