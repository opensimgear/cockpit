import importlib.util
from pathlib import Path
import unittest
spec = importlib.util.spec_from_file_location("provision", Path(__file__).resolve().parents[1] / "tools/provision.py")
provision = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provision)
class ProvisionTests(unittest.TestCase):
    def test_json_line_preserves_credentials_without_printing_them(self):
        record = {"command": "provision", "ssid": "wifi", "password": "quote\"line\nsecret", "unixMs": 1800000000000}
        encoded = provision.encode(record)
        self.assertEqual(encoded.count(b"\n"), 1)
        self.assertEqual(provision.json.loads(encoded), record)
    def test_large_records_are_rejected(self):
        with self.assertRaises(ValueError):
            provision.encode({"password": "x" * 9000})
    def test_only_bounded_status_tokens_are_displayed(self):
        self.assertEqual(provision.status_line(b'{"status":"live-display-only"}'), "live-display-only")
        for value in [b'not json', b'[]', b'{"token":"private"}', b'{"status":"secret with spaces"}', b'{"status":42}']:
            self.assertIsNone(provision.status_line(value))
class ArmingTests(unittest.TestCase):
    def status(self):
        return {"status":"live-disarmed", "touchAvailable":True, "readyForArm":True, "sessionId":"session-1", "generation":"1", "publicationHash":"sha256:"+"a"*64, "armEpoch":"4"}
    def test_arm_binds_only_public_current_identity(self):
        value=self.status(); value["token"]="do-not-copy"
        request=provision.arm_record(value)
        self.assertEqual(request["command"],"arm")
        self.assertEqual(request["armEpoch"],"4")
        self.assertNotIn("token",request)
    def test_arm_rejects_headless_held_and_malformed_challenges(self):
        for key,bad in [("touchAvailable",False),("readyForArm",False),("status","live-armed"),("sessionId","bad id"),("generation","0"),("generation","01"),("publicationHash","wrong"),("armEpoch","18446744073709551616"),("armEpoch",1)]:
            value=self.status();value[key]=bad
            with self.assertRaises(ValueError):provision.arm_record(value)
        value=self.status();del value["sessionId"]
        with self.assertRaises(ValueError):provision.arm_record(value)
    def test_status_exchange_ignores_buffered_records_and_requires_nonce(self):
        expected=self.status()
        class Port:
            def __init__(self):self.lines=[b'{"status":"live-armed","requestId":"old"}\n',b'garbage\n'];self.writes=[]
            def write(self,data):
                request=provision.json.loads(data);self.writes.append(request)
                response=dict(expected,requestId=request["requestId"])
                self.lines.append(provision.json.dumps(response).encode()+b"\n")
            def flush(self):pass
            def readline(self,size):
                self.asserted_size=size
                return self.lines.pop(0) if self.lines else b""
        port=Port();response=provision.request_status(port)
        self.assertEqual(provision.arm_record(response)["sessionId"],"session-1")
        self.assertEqual(port.writes[0]["command"],"status")
        self.assertEqual(port.asserted_size,8193)
    def test_terminal_status_never_confuses_connection_with_arming(self):
        self.assertFalse(provision.terminal_status("arm","live-disarmed"))
        self.assertFalse(provision.terminal_status("connect","live-armed"))
        self.assertTrue(provision.terminal_status("arm","live-armed"))
        self.assertTrue(provision.terminal_status("disarm","disarmed-reconnect-required"))
if __name__ == "__main__":
    unittest.main()
