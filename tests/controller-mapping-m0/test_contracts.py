import json
import unittest
from contracts import (DOCUMENT_LIMIT, FRAME_LIMIT, HideState, Integration, SourceStream,
                       counter, decode_frame, executable, plan_hiding)


class ContractTests(unittest.TestCase):
    def frame(self, document, **kw):
        return json.dumps(dict(schema=1, requestId="request", document=document, **kw),
                          ensure_ascii=False, separators=(",", ":")).encode()

    def test_max_document_fits_bounded_envelope(self):
        document = {"data": "x" * (DOCUMENT_LIMIT - len(b'{"data":""}'))}
        encoded = self.frame(document)
        self.assertLessEqual(len(encoded), FRAME_LIMIT)
        self.assertEqual(decode_frame(encoded)["document"], document)
        document["data"] += "x"
        with self.assertRaises(ValueError):
            decode_frame(self.frame(document))

    def test_invalid_envelopes_never_reach_import(self):
        cases = [b"x" * (FRAME_LIMIT + 1), b"\xff", b'[]', b'{"schema":1,"schema":1}',
                 b'{"schema":1,"requestId":"r","document":{"x":NaN}}',
                 self.frame({}, unexpected=True)]
        for data in cases:
            with self.subTest(data=data[:40]), self.assertRaises(ValueError):
                decode_frame(data)
        for field, value in [("schema", True), ("schema", 2), ("requestId", ""),
                             ("requestId", "x" * 81), ("requestId", 4), ("document", [])]:
            frame = dict(schema=1, requestId="r", document={})
            frame[field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                decode_frame(json.dumps(frame).encode())

    def test_counters_preserve_javascript_unsafe_integers(self):
        self.assertEqual(counter("18446744073709551615"), (1 << 64) - 1)
        for invalid in [1, True, "01", "0", "-1", "1.0", "١", "18446744073709551616"]:
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                counter(invalid)

    def test_features_are_independent_and_default_disabled(self):
        ready = frozenset({"create-virtual"})
        with self.assertRaisesRegex(ValueError, "disabled"):
            Integration(False, "ready", ready).require("create-virtual")
        self.assertEqual(Integration(True, "ready", ready).require("create-virtual"), "create-virtual")
        for state in ["missing", "unsupported", "setup-required", "reboot-required", "lost"]:
            with self.subTest(state=state), self.assertRaisesRegex(ValueError, state):
                Integration(True, state, ready).require("create-virtual")
        with self.assertRaisesRegex(ValueError, "unsupported"):
            Integration(True, "ready", frozenset({"suppress-input"})).require("hide-from-process")
        # vJoy readiness does not make hiding ready.
        with self.assertRaises(ValueError):
            Integration().require("hide-all")

    def test_source_gap_stale_replies_and_reconnect(self):
        source = SourceStream("stick", ["x", "button"])
        self.assertEqual(source.accept("stick", "1", "100", "baseline", {"x": 0, "button": 1}),
                         {"x": 0, "button": 1})
        self.assertEqual(source.accept("stick", "1", "101", "change", {"x": 12})["button"], 1)
        with self.assertRaisesRegex(ValueError, "gap"):
            source.accept("stick", "1", "103", "change", {"button": 0})
        self.assertFalse(source.armed)
        self.assertEqual(source.values, {})
        with self.assertRaises(ValueError):
            source.accept("stick", "1", "104", "baseline", {"x": 0, "button": 0})
        source.accept("stick", "2", "1", "baseline", {"x": 5, "button": 0})
        for args in [("stick", "1", "105"), ("other", "2", "2"), ("stick", "2", "1")]:
            with self.assertRaises(ValueError):
                source.accept(*args, "change", {"x": 30})
            self.assertTrue(source.armed)
            self.assertEqual(source.values["x"], 5)
        source.disconnect()
        with self.assertRaises(ValueError):
            source.accept("stick", "2", "2", "change", {"x": 30})

    def test_invalid_reports_are_atomic(self):
        source = SourceStream("stick", ["x", "button"])
        source.accept("stick", "1", "1", "baseline", {"x": 0, "button": 0})
        for changes in [{"x": 12, "unknown": 0}, {"x": 12, "button": 40000},
                        {"x": 1.5}, {"x": True}, {}]:
            with self.assertRaises(ValueError):
                source.accept("stick", "1", "2", "change", changes)
            self.assertEqual(source.values, {"x": 0, "button": 0})
            self.assertEqual(source.sequence, 1)

    def test_complete_baseline_required(self):
        source = SourceStream("stick", ["x", "button"])
        with self.assertRaises(ValueError):
            source.accept("stick", "1", "1", "baseline", {"x": 0})
        self.assertFalse(source.armed)


class PolicyTests(unittest.TestCase):
    capture = executable(r"C:\Cockpit\capture.exe")
    game = executable(r"C:\Games\game.exe")
    empty = HideState(False, False, frozenset(), frozenset())

    def test_normal_mode_keeps_capture_and_known_exceptions(self):
        state = HideState(False, False, frozenset({self.game}), frozenset())
        change = plan_hiding(state, [("physical:1", "hide-all", None)], self.capture)
        self.assertFalse(change.after.inverse)
        self.assertEqual(change.after.applications, frozenset({self.capture, self.game}))
        self.assertEqual(change.restore(change.after), state)

    def test_inverse_deny_list_does_not_become_normal_allow_exceptions(self):
        state = HideState(True, True, frozenset({self.game}), frozenset())
        change = plan_hiding(state, [("a", "hide-all", None)], self.capture)
        self.assertEqual(change.after.applications, frozenset({self.capture}))
        self.assertFalse(change.after.inverse)

    def test_common_process_policy_is_representable(self):
        change = plan_hiding(self.empty, [
            ("physical:1", "hide-from-process", self.game),
            ("physical:2", "hide-from-process", r"C:/GAMES/game.exe")], self.capture)
        self.assertTrue(change.after.inverse)
        self.assertNotIn(self.capture, change.after.applications)
        self.assertEqual(change.after.hidden, frozenset({"physical:1", "physical:2"}))

    def test_different_device_process_policies_fail_without_mutation(self):
        for selections in [
            [("a", "hide-all", None), ("b", "hide-from-process", self.game)],
            [("a", "hide-from-process", self.game), ("b", "hide-from-process", r"C:\Other.exe")],
            [("a", "hide-from-process", self.capture)],
            [("a", "suppress-input", None)],
            [("cockpit-virtual:1", "hide-all", None)],
            [("a", "hide-all", None), ("a", "hide-all", None)],
        ]:
            with self.subTest(selections=selections), self.assertRaises(ValueError):
                plan_hiding(self.empty, selections, self.capture)
        self.assertEqual(self.empty.hidden, frozenset())

    def test_external_hidden_devices_cannot_have_their_global_policy_changed(self):
        state = HideState(True, False, frozenset({self.capture}), frozenset({"external"}))
        with self.assertRaises(ValueError):
            plan_hiding(state, [("new", "hide-from-process", self.game)], self.capture)
        change = plan_hiding(state, [("new", "hide-all", None)], self.capture)
        self.assertIn("external", change.after.hidden)
        self.assertEqual(change.restore(change.after).hidden, frozenset({"external"}))

    def test_restore_preserves_external_changes_instead_of_overwriting_them(self):
        change = plan_hiding(self.empty, [("a", "hide-all", None)], self.capture)
        external = HideState(True, False, change.after.applications, frozenset({"a", "external"}))
        with self.assertRaisesRegex(ValueError, "external changes"):
            change.restore(external)
        self.assertIn("external", external.hidden)

    def test_paths_are_not_pids_or_relative_names(self):
        for value in ["game.exe", "1234", r"C:game.exe", r"C:\Games\..\game.exe", "", None]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                executable(value)
        self.assertEqual(executable(r"C:/Games/GAME.exe"), self.game)


if __name__ == "__main__":
    unittest.main()
