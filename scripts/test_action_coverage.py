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

integration_spec = importlib.util.spec_from_file_location(
    'integration_coverage', Path(__file__).with_name('integration-coverage.py'))
integration_coverage = importlib.util.module_from_spec(integration_spec)
integration_spec.loader.exec_module(integration_coverage)


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


class IntegrationCoverageTests(unittest.TestCase):
    def test_test_only_import_does_not_hide_following_production_functions(self):
        lines = ['#[cfg(test)]', 'use crate::fixture;', '',
                 'fn production() {}', '#[cfg(test)]', 'mod tests {', '}']
        self.assertEqual(integration_coverage.production_end(lines), 5)

    def test_external_test_module_attributes_are_excluded(self):
        lines = ['fn production() {}', '#[cfg(test)]',
                 '#[path = "other_tests.rs"]', 'mod tests;']
        self.assertEqual(integration_coverage.production_end(lines), 2)
        self.assertEqual(integration_coverage.production_end(lines[:1]), 2)

    def test_assetto_adapter_keeps_real_production_code_in_the_report(self):
        source = Path(__file__).resolve().parents[1] / 'src-tauri/src/games/telemetry/windows_source/mod.rs'
        lines = source.read_text(encoding='utf-8').splitlines()
        end = integration_coverage.production_end(lines)
        self.assertIn('fn consistent_copy(', '\n'.join(lines[:end - 1]))
        self.assertIn('mod tests;', lines[end:])
        self.assertGreater(end, 100)


if __name__ == '__main__':
    unittest.main()
