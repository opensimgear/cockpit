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
