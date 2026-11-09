"""Portable checks for the native workload's publication and scope evidence."""
import copy
import hashlib
import importlib.util
import math
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("native_latency_workload", Path(__file__).with_name("desktop-latency-native.py"))
workload = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workload)


class NativeWorkloadContractTests(unittest.TestCase):
    def test_keyboard_diagnostic_builds_only_key_routes(self):
        detail = {"resource": {"id": "peer"}, "ports": [{"contractHash": "button-contract",
            "descriptor": {"id": workload.BUTTON, "contract": {"delivery": "state", "schema": {"type": "boolean"}}}}]}
        with patch.object(workload, "current_companion_resource", return_value=detail):
            document, assignments = workload.profile_for(object(), ["a", "b"], {"publicationHash": "fixture"}, keys_only=True)
        self.assertEqual(document["processingNodes"], [])
        self.assertEqual([row["id"] for row in document["bindings"]], ["key-a", "key-b"])
        self.assertEqual([row["action"]["kind"] for row in document["desktopOutputs"][0]["actions"]], ["key-hold", "key-hold"])
        self.assertEqual([[port["profilePortId"] for port in row["ports"]] for row in assignments], [["button"], ["button"]])

    def test_keyboard_focus_requires_fresh_exact_window_without_cursor_positioning(self):
        identity = {"pid": 42, "created": "123", "executable": "native_receiver.exe"}
        prior = {"identity": identity, "focusRequests": 3}
        foreign = {**prior, "focusRequests": 4, "focusAfter": "100", "foregroundWindow": "200"}
        ready = {**foreign, "foregroundWindow": "100", "clientCursor": {"inside": False}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receiver.json"

            def wait(predicate, message, **_kwargs):
                if message.startswith("No complete"):
                    return predicate()
                self.assertEqual(path.with_suffix(".focus").read_text(), "focus")
                self.assertIsNone(predicate())
                return predicate()

            class Service:
                def profiles(self, _command):
                    return {"profileSession": {"state": "stopped", "owner": None}}

            with (patch.object(workload.mapping, "wait_for", side_effect=wait),
                  patch.object(workload.native, "receiver", side_effect=[prior, foreign, ready]),
                  patch.object(workload.native, "desktop_status", return_value={"recentApplication": identity})):
                result = workload.ownership.position_receiver(Service(), path, identity, keyboard_window="100")
        self.assertEqual(result["focusRequests"], 4)
        self.assertFalse(result["clientCursor"]["inside"])

    def test_pointer_proof_requires_marked_message_in_exact_foreground_without_capture(self):
        outcomes = [{"delivery": {"operationId": str(index), "actionId": action, "intent": {"kind": "pointer"}},
            "acknowledgement": {"ok": True, "uncertain": False, "skipped": False, "attempted": 1, "confirmed": 1}}
            for index, action in enumerate(("pointer-a", "pointer-b"), 1)]
        status = {"runtime": {"outcomes": outcomes}}
        move = {"message": 512, "injectedMarker": True, "foregroundWindow": "10", "captureWindow": "0"}
        report = {"focusAfter": "10", "observations": [move]}
        self.assertEqual(workload.pointer_load_proof(report, status, 0, 0)["receiverMarkedMouseMoves"], 1)
        for field, value in (("injectedMarker", False), ("foregroundWindow", "20"), ("captureWindow", "10")):
            with self.subTest(field=field):
                report["observations"] = [{**move, field: value}]
                self.assertIsNone(workload.pointer_load_proof(report, status, 0, 0))

    def test_pointer_proof_counts_both_participants_of_one_confirmed_submission(self):
        batch = {"dx": -1, "dy": -1, "operationIds": ["11", "12"]}
        outcomes = [{"delivery": {"operationId": operation, "actionId": action, "intent": {"kind": "pointer"}},
                     "acknowledgement": {"ok": True, "uncertain": False, "skipped": False, "attempted": count,
                                         "confirmed": count, "sequence": "5", "pointerBatch": batch}}
                    for operation, action, count in (("11", "pointer-a", 1), ("12", "pointer-b", 0))]
        status = {"runtime": {"outcomes": outcomes}}
        report = {"focusAfter": "10", "observations": [
            {"message": 512, "injectedMarker": True, "foregroundWindow": "10", "captureWindow": "0"}]}
        counts = workload.pointer_load_proof(report, status, 0, 10)["recentNativePointerCounts"]
        self.assertEqual([row["successfulSubmissions"] for row in counts.values()], [1, 1])
        self.assertEqual([row["nativeEventsConfirmed"] for row in counts.values()], [1, 0])
        outcomes[1]["acknowledgement"]["sequence"] = "6"
        with self.assertRaises(AssertionError):
            workload.pointer_load_proof(report, status, 0, 10)
        del outcomes[0]
        self.assertIsNone(workload.pointer_load_proof(report, status, 0, 10))

    def test_neutral_owner_check_distinguishes_renewal_from_an_inflight_input(self):
        owners = [{"actionId": action, "controls": []}
            for action in ("key-a", "key-b", "pointer-a", "pointer-b")]
        backend = {"inFlight": None, "owners": owners}
        runtime = {"state": "active", "waitingNeutral": [], "dispatchPending": True, "backend": backend}
        with patch.object(workload.native, "desktop_status", return_value={"runtime": runtime}):
            self.assertEqual(workload.neutral_owners(object()), owners)
            backend["inFlight"] = {"sequence": "7"}
            self.assertIsNone(workload.neutral_owners(object()))
            backend["inFlight"] = None
            owners[0]["controls"] = [{"kind": "key", "key": 4}]
            self.assertIsNone(workload.neutral_owners(object()))

    def test_partial_failure_retains_confirmed_receiver_injection_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receiver.json"
            self.assertFalse(workload.receiver_injection_observed([path]))
            path.write_text(json.dumps({"observations": [{"injectedMarker": False}]}))
            self.assertFalse(workload.receiver_injection_observed([path]))
            path.write_text(json.dumps({"observations": [{"injectedMarker": True, "message": 256}]}))
            self.assertTrue(workload.receiver_injection_observed([path]))

    def test_key_settlement_waits_through_a_partial_receiver_write(self):
        edge = {"injectedMarker": True, "message": 256, "scanCode": 30, "extended": 0}
        record = {"scanCode": 30, "actionId": "key-a", "keyChanges": [], "finalPressed": False}
        backend = {"uncertain": False, "error": None, "inFlight": None, "possibleHeld": [], "held": [],
                   "owners": [{"actionId": "key-a", "controls": []}]}
        status = {"runtime": {"error": None, "scopeError": None, "state": "active", "waitingNeutral": [],
                              "backend": backend}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receiver.json"
            path.write_text('{"identity":', encoding="utf-8")

            def wait(predicate, _message, **_kwargs):
                self.assertIsNone(predicate())
                path.write_text(json.dumps({"identity": {"pid": 42}, "observations": [edge]}), encoding="utf-8")
                return predicate()

            with (patch.object(workload.mapping, "wait_for", side_effect=wait),
                  patch.object(workload.native, "desktop_status", return_value=status),
                  patch.object(workload.native, "compact_desktop", return_value={})):
                result = workload.settle_keys(object(), path, [record], {"30": [edge]})
        self.assertEqual(result["keyCounts"][0]["warmupEdgesExcluded"], 1)

    def test_supported_publication_and_signed_waveform_preserve_exact_neutral(self):
        publication = workload.compiled_publication()
        self.assertEqual(publication["publicationHash"], "sha256:" + hashlib.sha256(publication["canonicalDocument"].encode()).hexdigest())
        self.assertEqual(math.floor(workload.AXIS_NEUTRAL * 32767 + 0.5) * workload.AXIS_FACTOR + workload.AXIS_OFFSET, 0)
        for tick in range(workload.RATE * workload.SECONDS):
            signed = round(0.65 * math.sin((tick + 1) * math.pi / 40), 6)
            wire = workload.axis_wire(signed)
            self.assertTrue(0 <= wire <= 1)
            actual = math.floor(wire * 32767 + 0.5) * workload.AXIS_FACTOR + workload.AXIS_OFFSET
            self.assertLessEqual(abs(actual - signed), 2.65 / 32768)
        with self.assertRaises(AssertionError):
            workload.axis_boundary({"schema": {"type": "integer", "min": -32768, "max": 32767}})

    def test_receiver_identity_and_reference_are_required_before_a_profile_write(self):
        identity = {"pid": 10, "created": "20", "executable": "receiver.exe"}
        saved = {"id": "owned", "revision": 1, "desktopOutputs": [{"id": "desktop"}]}
        cases = [{**identity, field: value} for field, value in
                 [("pid", 11), ("created", "21"), ("executable", "other.exe")]]
        captures = [{"recentApplication": value, "capturedApplicationReference": "reference"} for value in cases]
        captures.append({"recentApplication": identity, "capturedApplicationReference": ""})
        for capture in captures:
            with self.subTest(capture=capture), patch.object(workload.mapping, "state", return_value=capture), patch.object(workload.mapping, "save") as save:
                with self.assertRaises(AssertionError):
                    workload.capture_profile_scope(object(), saved, [], identity)
                save.assert_not_called()

    def test_each_phase_binds_its_exact_receiver_with_a_revision_guard(self):
        identity = {"pid": 10, "created": "20", "executable": "receiver.exe"}
        original = {"id": "owned", "revision": 1, "desktopOutputs": [{"id": "desktop"}]}
        saved = copy.deepcopy(original)
        compiled = {"readiness": "ready", "requirements": {"desktopInput": True, "virtualDevices": False, "visibility": False}, "budgets": {"virtualTargets": 0}}
        references = iter(["first-receiver", "second-receiver"])
        expected_revisions = []

        def state(service, command, key, **fields):
            if command == "profile_desktop_capture_application":
                return {"recentApplication": identity, "capturedApplicationReference": next(references)}
            self.assertEqual(command, "profile_compile")
            self.assertEqual(fields["document"]["id"], "owned")
            return compiled

        def save(service, document, assignments, expected):
            expected_revisions.append(expected)
            self.assertEqual(document["revision"], expected)
            return {**document, "revision": expected + 1}

        with patch.object(workload.mapping, "state", side_effect=state), patch.object(workload.mapping, "save", side_effect=save):
            for reference in ["first-receiver", "second-receiver"]:
                saved, _, _ = workload.capture_profile_scope(object(), saved, [], identity)
                self.assertEqual(saved["desktopOutputs"][0]["scope"], {"kind": "application", "applicationReference": reference})
        self.assertEqual(expected_revisions, [1, 2])
        self.assertEqual(saved["revision"], 3)
        self.assertNotIn("scope", original["desktopOutputs"][0])


if __name__ == "__main__":
    unittest.main()
