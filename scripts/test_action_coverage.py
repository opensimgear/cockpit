import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    'action_coverage', Path(__file__).with_name('action-coverage.py'))
coverage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage)


class ActionCoverageTests(unittest.TestCase):
    def test_mixed_cargo_output_keeps_only_executable_test_artifacts(self):
        def artifact(executable, test):
            return json.dumps({'reason': 'compiler-artifact', 'executable': executable,
                               'profile': {'test': test}})
        output = '\n'.join([
            '   Compiling osg-cockpit v0.1.0',
            artifact(None, False),
            'warning: a native dependency emitted a diagnostic',
            artifact('/target/debug/helper', False),
            artifact('/target/debug/deps/osg_cockpit-test', True),
            '    Finished `test` profile [unoptimized + debuginfo]',
        ])
        self.assertEqual(coverage.cargo_test_binaries(output),
                         ['/target/debug/deps/osg_cockpit-test'])
        with self.assertRaises(json.JSONDecodeError):
            coverage.cargo_test_binaries('{malformed cargo record}')

    def test_failed_cargo_retains_diagnostics_and_never_accepts_its_artifact(self):
        output = ('{"reason":"compiler-artifact","executable":"/stale-test",'
                  '"profile":{"test":true}}\nerror[E0425]: compilation failed\n')
        result = subprocess.CompletedProcess(['cargo', 'test'], 101, stdout=output)
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'failed-cargo.log'
            diagnostic = io.StringIO()
            with patch.object(coverage.subprocess, 'run', return_value=result), \
                    contextlib.redirect_stderr(diagnostic):
                with self.assertRaises(subprocess.CalledProcessError) as failure:
                    coverage.command(['cargo', 'test'], directory, log_path=log)
            self.assertEqual(failure.exception.returncode, 101)
            self.assertEqual(log.read_text(), output)
            self.assertEqual(diagnostic.getvalue(), output)


if __name__ == '__main__':
    unittest.main()
