import importlib.util
import os
from pathlib import Path
import tempfile
import unittest

from native_coverage_sources import expand_includes, measure_module, production_sources, source_stamp


class IncludeCoverageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def test_recursive_relative_includes_are_measured_once(self):
        facade = self.write("mod.rs", 'include!("nested/a.rs");\ninclude!("nested/b.rs");\n')
        a = self.write("nested/a.rs", 'include!("b.rs");\nfn a() {}\n')
        b = self.write("nested/b.rs", 'fn b() {}\n')
        self.assertEqual(production_sources(facade), [facade, a, b])

    def test_comments_and_literals_do_not_create_source_dependencies(self):
        source = self.write("mod.rs", '// include!("missing.rs");\n'
                            'const TEXT: &str = r#"include!("also-missing.rs");"#;\n'
                            '/* include!("other-missing.rs"); */\nfn real() {}\n')
        self.assertEqual(expand_includes([source]), [source])

    def test_test_includes_are_stamped_but_not_in_the_denominator(self):
        source = self.write("mod.rs", 'include!("production.rs");\ninclude!("tests.rs");\n')
        production = self.write("production.rs", 'fn production() {}\n')
        tests = self.write("tests.rs", '#[cfg(test)]\nmod tests { include!("cases.rs"); }\n')
        cases = self.write("cases.rs", 'fn regression() {}\n')
        self.assertEqual(production_sources(source), [source, production])
        before = source_stamp([source], self.root)
        self.assertIn(str(cases.relative_to(self.root)), before)
        self.assertIn(str(tests.relative_to(self.root)), before)
        cases.write_text('fn changed_regression() {}\n', encoding="utf-8")
        self.assertNotEqual(before, source_stamp([source], self.root))

    def test_production_after_interleaved_test_helpers_is_measured(self):
        source = self.write("mod.rs", '#[cfg(test)]\nuse crate::fixture;\n'
                            '#[cfg(test)]\nfn helper() {}\n'
                            'include!("production.rs");\nfn after() {}\n')
        fragment = self.write("production.rs", 'fn real() {}\n')
        annotations = {source: ' 4|100|fn helper() {}\n 6|0|fn after() {}\n',
                       fragment: ' 1|1|fn real() {}\n'}
        result, _ = measure_module(source, annotations.__getitem__, self.root)
        self.assertEqual((result["covered"], result["lines"]), (1, 2))
        self.assertEqual(result["percent"], 50)

    def test_uncovered_fragment_lines_cannot_hide_behind_a_covered_facade(self):
        source = self.write("mod.rs", 'fn facade() {}\ninclude!("fragment.rs");\n')
        fragment = self.write("fragment.rs", 'fn uncovered() {\n    work();\n}\n')
        annotations = {source: ' 1|1k|fn facade() {}\n', fragment:
                       ' 1|0|fn uncovered() {\n 2|0|    work();\n 3|0|}\n'}
        result, _ = measure_module(source, annotations.__getitem__, self.root)
        self.assertEqual((result["covered"], result["lines"]), (1, 4))
        self.assertEqual(result["percent"], 25)
        self.assertEqual(result["fragments"]["fragment.rs"]["lines"], 3)

    def test_declaration_facade_can_aggregate_real_fragment_coverage(self):
        source = self.write("mod.rs", 'include!("fragment.rs");\n')
        fragment = self.write("fragment.rs", 'fn work() {}\n')
        result, _ = measure_module(source, lambda path: ' 1|1|fn work() {}\n' if path == fragment else '', self.root)
        self.assertEqual((result["covered"], result["lines"]), (1, 1))
        self.assertEqual(result["fragments"]["mod.rs"]["lines"], 0)

    def test_missing_llvm_evidence_for_an_executable_fragment_fails(self):
        source = self.write("mod.rs", 'fn facade() {}\ninclude!("fragment.rs");\n')
        self.write("fragment.rs", 'fn missing() {}\n')
        with self.assertRaisesRegex(RuntimeError, "executable fragment.*fragment.rs"):
            measure_module(source, lambda path: ' 1|1|fn facade() {}\n' if path == source else '')

    def test_inactive_host_platform_items_are_not_executable_fragments(self):
        inactive = 'not(windows)' if os.name == 'nt' else 'windows'
        source = self.write("mod.rs", 'fn active() {}\ninclude!("inactive.rs");\n')
        self.write("inactive.rs", f'#[cfg({inactive})]\nmod inactive {{ fn function() {{}} }}\n')
        result, _ = measure_module(source, lambda _: ' 1|1|fn active() {}\n')
        self.assertEqual(result["lines"], 1)

    def test_missing_and_cyclic_includes_fail_closed(self):
        source = self.write("mod.rs", 'include!("missing.rs");\n')
        with self.assertRaisesRegex(RuntimeError, "Missing Rust coverage source"):
            production_sources(source)
        self.write("missing.rs", 'include!("mod.rs");\n')
        with self.assertRaisesRegex(RuntimeError, "Cyclic Rust include"):
            production_sources(source)


class RepositoryCoverageTests(unittest.TestCase):
    def test_all_split_production_fragments_are_assigned_to_their_owner(self):
        root = Path(__file__).resolve().parents[1] / "src-tauri/src"
        owners = {
            "profile_resources_game.rs": ["resources_publication.rs", "resources_binding.rs"],
            "profile_resource_sources.rs": ["inventory_collection.rs", "inventory_devices.rs", "inventory_can.rs"],
            "profile_session_games.rs": ["session_prepare.rs", "session_read.rs", "session_delivery.rs"],
            "game_discovery.rs": ["catalogue.rs", "manifests.rs", "launch.rs"],
        }
        for owner, fragments in owners.items():
            with self.subTest(owner=owner):
                sources = production_sources(root / owner)
                self.assertTrue(set(fragments).issubset({path.name for path in sources}))
                self.assertFalse(any(path.name.startswith("tests") for path in sources))

    def test_game_gate_covers_discovery_review_and_binding_boundaries(self):
        spec = importlib.util.spec_from_file_location("game_coverage", Path(__file__).with_name("game-library-coverage.py"))
        gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(gate)
        for name in ["games/library/disk.rs", "games/review/mod.rs", "games/review/reconcile.rs",
                     "games/review/worker.rs", "games/storage/installations.rs", "game_metadata_api.rs",
                     "game_discovery.rs", "game_context_runtime.rs", "mcp_tools/game_library.rs"]:
            self.assertIn(name, gate.MODULES)
        for query in ["game_discovery", "games::discovery_review", "games::metadata_api", "game_installations"]:
            self.assertIn(query, gate.TEST_FILTERS)


if __name__ == "__main__":
    unittest.main()
