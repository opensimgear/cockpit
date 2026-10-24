import importlib.util
from pathlib import Path
import unittest
import tempfile


spec = importlib.util.spec_from_file_location(
    'integration_coverage', Path(__file__).with_name('integration-coverage.py'))
coverage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage)


class IntegrationCoverageTests(unittest.TestCase):
    def test_sqlite_schema_changes_invalidate_the_source_snapshot(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'crates/persistence/src'
            source.mkdir(parents=True)
            schema = source / 'schema.sql'
            schema.write_text('CREATE TABLE records(value BLOB);', encoding='utf-8')
            before = coverage.source_stamp(root)
            schema.write_text('CREATE TABLE records(value TEXT);', encoding='utf-8')
            self.assertNotEqual(before, coverage.source_stamp(root))
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
        source = Path(__file__).resolve().parents[1] / 'src-tauri/src/games/telemetry/windows_source/mod.rs'
        lines = source.read_text(encoding='utf-8').splitlines()
        end = coverage.production_end(lines)
        acquisition = next(i + 1 for i, line in enumerate(lines)
                           if line.startswith('fn consistent_copy_for_game('))
        self.assertLess(acquisition, end)
        self.assertEqual(lines[end], 'mod tests;')

class ProductionBoundaryTests(unittest.TestCase):
    def test_test_only_imports_do_not_hide_production_functions(self):
        lines = ['use native::Reader;', '#[cfg(test)]', 'use native::{', '    Fake,', '};',
                 'fn production() {}', '#[cfg(test)]', 'mod tests {}']
        self.assertEqual(coverage.production_end(lines), 7)

    def test_path_test_modules_and_test_helpers_keep_their_exclusion(self):
        for section in ['#[path = "tests.rs"]', 'fn test_helper() {}', 'mod tests {}']:
            self.assertEqual(coverage.production_end(['fn production() {}', '#[cfg(test)]', section]), 2)

    def test_modules_without_a_test_section_measure_all_production(self):
        lines = ['#[cfg(test)]', '', 'pub use fixtures::Fake;', 'fn production() {}']
        self.assertEqual(coverage.production_end(lines), 5)


if __name__ == '__main__':
    unittest.main()
