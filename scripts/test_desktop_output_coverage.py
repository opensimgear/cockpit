"""Portable parser regressions; these fixtures never construct native output."""
import importlib.util
from pathlib import Path
import re
import unittest

spec = importlib.util.spec_from_file_location("desktop_output_coverage",
    Path(__file__).with_name("desktop-output-coverage.py"))
coverage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage)


def annotated(source, covered=()):
    return "\n".join(f"{number}|{int(number in covered)}|{line}"
        for number, line in enumerate(source.splitlines(), 1))


class CoverageParserTests(unittest.TestCase):
    def test_actual_coordinator_keeps_send_submit_and_persistence(self):
        source = (Path(__file__).resolve().parents[1] /
            "crates/desktop-output/src/coordinator.rs").read_text(encoding="utf-8")
        lines = source.splitlines()
        methods = [number for number, line in enumerate(lines, 1)
            if re.match(r"\s*(?:pub )?fn (?:send|submit|persist_inner)\(", line)]
        self.assertEqual(len(methods), 3)
        first_test = next(number for number, line in enumerate(lines, 1)
            if line.strip() == "#[cfg(test)]")
        self.assertTrue(all(number > first_test for number in methods))
        rows = "\n".join(["1|1|production", *[f"{number}|0|method" for number in methods]])
        counts = coverage.production_counts(source, rows)
        self.assertEqual(counts, {1: True, **dict.fromkeys(methods, False)})
        self.assertEqual(sum(counts.values()) / len(counts), 0.25)
        test_fields = [number for number, line in enumerate(lines, 1)
            if "persist_attempts: std::cell::Cell" in line]
        self.assertTrue(test_fields)
        self.assertTrue(set(test_fields).issubset(coverage.test_only_lines(source)))

    def test_fields_blocks_helpers_and_later_uncovered_method(self):
        source = '''struct Example {
    #[cfg(test)]
    calls: usize,
}
impl Example {
    #[cfg(test)]
    fn helper(&self) { let _ = "} # [cfg(test)]"; }
    fn uncovered(&self) {
        #[cfg(test)]
        { self.helper(); /* { /* nested */ } */ }
        production();
        let _value = Example {
            #[cfg(test)]
            calls: usize::from(true),
        };
    }
}
'''
        excluded = coverage.test_only_lines(source)
        self.assertTrue({2, 3, 6, 7, 9, 10, 13, 14}.issubset(excluded))
        counts = coverage.production_counts(source, annotated(source, excluded))
        self.assertIn(8, counts)
        self.assertIn(11, counts)
        self.assertFalse(counts[8])
        self.assertFalse(any(counts.values()))

    def test_module_path_attributes_do_not_truncate_later_production(self):
        source = '''#[cfg(test)]
#[path = "coverage_tests.rs"]
mod tests;
fn later_uncovered() {}
#[cfg(test)]
mod inline_tests { fn helper() {} }
fn final_uncovered() {}
'''
        counts = coverage.production_counts(source, annotated(source, (1, 2, 3, 5, 6)))
        self.assertEqual(counts, {4: False, 7: False})

    def test_test_only_use_is_one_item_and_strings_cannot_create_attributes(self):
        source = '''#[cfg(test)]
use crate::{first, second};
fn production() {
    let _ = r###"#[cfg(test)] mod tests { }"###;
    let _ = '{';
    uncovered();
}
'''
        counts = coverage.production_counts(source, annotated(source, (1, 2)))
        self.assertNotIn(2, counts)
        self.assertIn(6, counts)
        self.assertFalse(counts[6])

    def test_mixed_lines_and_generic_field_suffix_are_retained_conservatively(self):
        source = '''fn production() { #[cfg(test)] { helper(); } uncovered(); }
struct Example {
    #[cfg(test)]
    values: Map<First, Second>,
}
fn later_uncovered() {}
'''
        counts = coverage.production_counts(source, annotated(source))
        self.assertIn(1, counts)
        self.assertIn(4, counts)
        self.assertIn(6, counts)

    def test_llvm_hot_count_abbreviations_still_count_as_covered(self):
        self.assertEqual(coverage.production_counts("first();\nsecond();\n", "1|1.2k|first\n2|0|second"),
            {1: True, 2: False})


if __name__ == "__main__":
    unittest.main()
