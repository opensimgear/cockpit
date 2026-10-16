import importlib.util
import json
from pathlib import Path
import unittest

path = Path(__file__).resolve().parents[1] / "tools/baseline.py"
spec = importlib.util.spec_from_file_location("companion_baseline", path)
baseline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(baseline)

class BaselineTests(unittest.TestCase):
    def summary(self, **changes):
        value = {key: 0 for key in baseline.SUMMARY_FIELDS}
        value.update(status="diagnostics-summary", version=1, requestId="capture-1", boardProfile="sunton-3248s035r", firmwareVersion="0.1.0",
                     stackMinBytes=[100,200,300], stackSamples=[1,1,1], sampledAtUptimeMs=2000000, windowStartedUptimeMs=1000)
        value.update(changes)
        return value

    def metric(self, name, **changes):
        value = {key: 0 for key in baseline.METRIC_FIELDS}
        value.update(status="diagnostics-metric", version=1, requestId="capture-1", metric=name)
        value.update(changes)
        return value

    def feed(self, capture, value):
        capture.feed(json.dumps(value).encode())

    def test_complete_and_redacted(self):
        capture = baseline.Capture("capture-1")
        capture.feed(b'{"status":"paired","token":"never-save-this"}')
        self.feed(capture, self.summary(password="never-save-this"))
        for name in baseline.METRICS:
            self.feed(capture, self.metric(name, secret="never-save-this"))
        self.feed(capture, {"status":"diagnostics-complete", "version":1,"requestId":"capture-1"})
        result = capture.result(1800)
        self.assertEqual(result["windowMs"],1999000)
        self.assertNotIn("never-save-this",json.dumps(result))
        with self.assertRaises(ValueError):
            capture.result(2000)

    def test_incomplete_duplicate_bad_counters_and_correlation(self):
        capture = baseline.Capture("capture-1")
        self.feed(capture, self.summary(requestId="other"))
        self.assertIsNone(capture.summary)
        capture.feed(b'invalid JSON')
        capture.feed(b'[]')
        capture.feed(b'x'*2049)
        with self.assertRaises(ValueError):
            capture.result()
        self.feed(capture, self.summary())
        with self.assertRaises(ValueError):
            self.feed(capture,{"status":"diagnostics-complete","version":1,"requestId":"capture-1"})
        for counter in (-1,1.5,True,18446744073709551616):
            with self.assertRaises(ValueError):
                self.feed(capture,self.metric("render",count=counter))
        self.feed(capture,self.metric("render"))
        with self.assertRaises(ValueError):
            self.feed(capture,self.metric("render"))
        for value in (self.summary(stackSamples=[]),self.summary(boardProfile='bad"profile'),self.summary(requestId='bad id')):
            with self.assertRaises(ValueError):
                self.feed(capture,value)

if __name__ == "__main__":
    unittest.main()
