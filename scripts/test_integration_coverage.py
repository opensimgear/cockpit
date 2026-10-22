import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('integration_coverage', Path(__file__).with_name('integration-coverage.py'))
coverage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage)


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
