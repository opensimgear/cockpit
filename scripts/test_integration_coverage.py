import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location(
    'integration_coverage', Path(__file__).with_name('integration-coverage.py'))
coverage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage)


class IntegrationCoverageTests(unittest.TestCase):
    def test_test_only_import_does_not_hide_production_functions(self):
        lines = ['#[cfg(test)]', 'use crate::fixture;', 'fn production() {}',
                 '#[cfg(test)]', 'mod tests {', 'fn regression() {}', '}']
        self.assertEqual(coverage.production_end(lines), 4)

    def test_external_test_module_attributes_are_supported(self):
        lines = ['fn production() {}', '#[cfg(test)]', '#[path = "tests.rs"]',
                 'mod tests;']
        self.assertEqual(coverage.production_end(lines), 2)

    def test_no_test_module_keeps_the_entire_production_file(self):
        lines = ['#[cfg(test)]', 'use crate::fixture;', 'fn production() {}']
        self.assertEqual(coverage.production_end(lines), 4)

    def test_assetto_adapter_includes_production_before_its_actual_tests(self):
        source = Path(__file__).resolve().parents[1] / 'src-tauri/src/acr_windows.rs'
        lines = source.read_text(encoding='utf-8').splitlines()
        end = coverage.production_end(lines)
        acquisition = next(i + 1 for i, line in enumerate(lines)
                           if line.startswith('fn consistent_copy_for_game('))
        self.assertLess(acquisition, end)
        self.assertEqual(lines[end], 'mod tests {')


if __name__ == '__main__':
    unittest.main()
