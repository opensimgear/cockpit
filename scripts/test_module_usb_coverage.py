"""Coverage evidence belongs to each test invocation, even when Windows reuses PIDs."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "module_usb_coverage", Path(__file__).with_name("module-usb-coverage.py"))
coverage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(coverage)


class RepositorySourceTests(unittest.TestCase):
    def test_default_gate_tracks_only_existing_current_sources(self):
        root = Path(__file__).resolve().parents[1]
        paths = coverage.tracked_source_paths(root, coverage.MODULES, False)
        self.assertTrue(paths)
        self.assertIn(root / "src-tauri/src/companion/storage/panels.rs", paths)
        self.assertIn(root / "src-tauri/src/profile_session/companion_rebind.rs", paths)
        self.assertIn(root / "src-tauri/src/profile_session/companion_rebind_tests.rs", paths)
        self.assertIn("profile_session_host::companion::rebind::tests", coverage.TEST_FILTERS)
        for path in paths:
            with self.subTest(path=str(path.relative_to(root))):
                self.assertTrue(path.is_file(), "Coverage must pin the current production/test source")


class InvocationEvidenceTests(unittest.TestCase):
    def run_filters(self, callback, filters=("first", "second")):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        output = Path(directory.name)
        profiles = output / "profiles"
        profiles.mkdir()
        with patch.object(coverage.subprocess, "run", side_effect=callback):
            result = coverage.run_test_filters("native-tests.exe", filters, output,
                                               dict(os.environ), profiles, output)
        return output, result

    @staticmethod
    def profile(kwargs, data=b"nonempty profile"):
        # The same LLVM module signature and reused PID in every subprocess.
        path = Path(kwargs["env"]["LLVM_PROFILE_FILE"].replace("%m", "module").replace("%p", "42"))
        path.write_bytes(data)
        return path

    def test_reused_pid_and_duplicate_filters_keep_separate_evidence(self):
        destinations = []

        def execute(args, **kwargs):
            destinations.append(self.profile(kwargs))
            return subprocess.CompletedProcess(args, 0, "one matching native test passed\n")

        output, (files, invocations) = self.run_filters(execute, ("same", "same"))
        self.assertEqual(len(files), 2)
        self.assertEqual(files[0].name, files[1].name)
        self.assertNotEqual(files[0].parent, files[1].parent)
        self.assertEqual(files, destinations)
        self.assertEqual([item["testFilter"] for item in invocations], ["same", "same"])
        self.assertTrue(all(item["profiles"][0]["bytes"] > 0 for item in invocations))
        self.assertEqual((output / "tests.log").read_text().count("native test passed"), 2)

    def test_previous_evidence_cannot_mask_a_missing_second_invocation(self):
        calls = []

        def execute(args, **kwargs):
            calls.append(args[1])
            if len(calls) == 1:
                self.profile(kwargs)
                extra = Path(kwargs["env"]["LLVM_PROFILE_FILE"].replace("%m", "second-module").replace("%p", "42"))
                extra.write_bytes(b"another profile from the first process")
            return subprocess.CompletedProcess(args, 0, "passed\n")

        with self.assertRaisesRegex(RuntimeError, "'second'.*missing or empty"):
            self.run_filters(execute)
        self.assertEqual(calls, ["first", "second"])

    def test_empty_profile_is_rejected_even_beside_a_nonempty_one(self):
        def execute(args, **kwargs):
            path = self.profile(kwargs)
            path.with_name("test-empty-42.profraw").touch()
            return subprocess.CompletedProcess(args, 0, "passed\n")

        with self.assertRaisesRegex(RuntimeError, "'first'.*missing or empty"):
            self.run_filters(execute)

    def test_failed_process_preserves_output_and_exit_status(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        output = Path(directory.name)
        profiles = output / "profiles"
        profiles.mkdir()
        result = subprocess.CompletedProcess(["native-tests.exe"], 7, "actual native failure details\n")
        errors = io.StringIO()
        with patch.object(coverage.subprocess, "run", return_value=result), contextlib.redirect_stderr(errors):
            with self.assertRaises(subprocess.CalledProcessError) as failure:
                coverage.run_test_filters("native-tests.exe", ["failed"], output, {}, profiles, output)
        self.assertEqual(failure.exception.returncode, 7)
        self.assertIn("actual native failure details", errors.getvalue())
        self.assertIn("actual native failure details", (output / "tests.log").read_text())

    def test_hung_filter_is_killed_and_retains_partial_output(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            profiles = output / "profiles"
            profiles.mkdir()
            script = output / "hung.py"
            script.write_text(
                "import time\nprint('test blocked_launch ...', flush=True)\ntime.sleep(60)\n",
                encoding="utf-8")
            errors = io.StringIO()
            with contextlib.redirect_stderr(errors):
                with self.assertRaisesRegex(RuntimeError, "hung.py.*timed out"):
                    coverage.run_test_filters(sys.executable, [str(script)], output,
                                              dict(os.environ), profiles, output,
                                              timeout_seconds=1)
            log = (output / "tests.log").read_text()
            self.assertIn(str(script), log)
            self.assertIn("test blocked_launch ...", log)
            self.assertIn("timed out after 1s", log)
            self.assertIn("test blocked_launch ...", errors.getvalue())


if __name__ == "__main__":
    unittest.main()
