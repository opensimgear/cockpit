import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import unittest

spec = importlib.util.spec_from_file_location("companion_mapping_acceptance", Path(__file__).with_name("companion-mapping-acceptance.py"))
acceptance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(acceptance)


class Transport:
    def __init__(self, data):
        self.data = bytearray(data)
        self.sent = []

    def recv(self, size):
        result = bytes(self.data[:size])
        del self.data[:size]
        return result

    def sendall(self, data):
        self.sent.append(data)


def server_frame(value):
    data = json.dumps(value, separators=(",", ":")).encode()
    return (bytes([0x81, len(data)]) if len(data) < 126 else bytes([0x81, 126]) + struct.pack("!H", len(data))) + data


def socket_for(data):
    socket = acceptance.Socket.__new__(acceptance.Socket)
    socket.socket = Transport(data)
    socket.buffer = bytearray()
    return socket


class AcceptanceTests(unittest.TestCase):
    def test_client_frames_are_masked_and_bounded_for_both_length_encodings(self):
        for length in [1, 125, 126, acceptance.FRAME_BYTES]:
            payload = b"a" * length
            encoded = acceptance.encoded_frame(payload)
            self.assertEqual(encoded[0], 0x81)
            self.assertTrue(encoded[1] & 0x80)
            offset = 2 if length < 126 else 4
            if offset == 4:
                self.assertEqual(struct.unpack("!H", encoded[2:4])[0], length)
            mask = encoded[offset:offset + 4]
            self.assertEqual(bytes(value ^ mask[index % 4] for index, value in enumerate(encoded[offset + 4:])), payload)
        with self.assertRaises(AssertionError):
            acceptance.encoded_frame(b"a" * (acceptance.FRAME_BYTES + 1))

    def test_server_frames_fail_before_accepting_fragmentation_masking_or_large_payloads(self):
        self.assertEqual(socket_for(server_frame({"ok": True})).frame(), {"ok": True})
        for header in [b"\x01\x00", b"\x81\x80", b"\x81\x7f" + struct.pack("!Q", acceptance.FRAME_BYTES + 1)]:
            with self.assertRaises(AssertionError):
                socket_for(header).frame()
        with self.assertRaises(RuntimeError):
            socket_for(b"").frame()

    def test_publication_transfer_preserves_bytes_and_rejects_corruption_or_reordering(self):
        payload = b'{"canonicalDocument":"exact bytes"}'
        start = {"type": "transfer-start", "id": "transfer-a", "bytes": len(payload), "sha256": "sha256:" + hashlib.sha256(payload).hexdigest()}
        chunk = {"type": "transfer-chunk", "id": "transfer-a", "index": 0, "base64": base64.b64encode(payload).decode()}
        end = {"type": "transfer-end", "id": "transfer-a"}
        self.assertEqual(socket_for(b"".join(server_frame(item) for item in [start, chunk, end])).request({}), json.loads(payload))
        for bad in [{**chunk, "index": 1}, {**chunk, "base64": base64.b64encode(b"x" * len(payload)).decode()}, {**chunk, "id": "other"}]:
            with self.assertRaises(AssertionError):
                socket_for(b"".join(server_frame(item) for item in [start, bad, end])).request({})

    def test_recording_page_retries_only_explicit_native_busy_results(self):
        class Service:
            def __init__(self, replies):
                self.replies = iter(replies)

            def request(self, command, **payload):
                self.command, self.payload = command, payload
                return next(self.replies)

        busy = {"ok": False, "error": "This companion's feeder is busy; retry its recording page without waiting for native I/O."}
        expected = {"instanceId": "instance-a", "records": []}
        service = Service([busy, {"ok": True, "state": {"mappings": {"companionRoute": expected}}}])
        self.assertEqual(acceptance.route(service, "instance-a", "recording", after="2"), expected)
        self.assertEqual(service.payload, {"companion": {"instanceId": "instance-a", "action": "recording", "after": "2"}})
        for action, failure in [("inspect", busy), ("recording", {"ok": False, "error": "Invalid cursor"})]:
            with self.assertRaises(AssertionError):
                acceptance.route(Service([failure]), "instance-a", action)
    def test_expected_reports_check_exact_authored_controls_and_nonzero_axis_fallback(self):
        document = {"ui": {"controls": [{"id": "axis", "kind": "axis", "fallback": 0.25}, {"id": "button", "kind": "button"}, {"id": "hat", "kind": "hat"}]}, "targets": [{"id": "logical"}], "bindings": [{"controlId": kind, "targetId": "logical", "kind": kind, "index": 0} for kind in ["axis", "button", "hat"]]}
        publication = {"canonicalDocument": json.dumps(document)}
        profile = {"targets": [{"id": "instance-target", "layout": {"axes": [{"fallback": 8192}], "buttons": 1, "hats": 1}}]}
        self.assertEqual(acceptance.expected_sample(profile, publication), {"instance-target": {"axes": [24575], "buttons": [True], "hats": [7]}})
        profile["targets"][0]["layout"]["axes"][0]["fallback"] = 0
        with self.assertRaisesRegex(AssertionError, "fallback"):
            acceptance.expected_sample(profile, publication)
        self.assertEqual(acceptance.axis_value(-0.5), -16384)


if __name__ == "__main__":
    unittest.main()
