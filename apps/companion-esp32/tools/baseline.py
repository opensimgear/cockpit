"""Capture bounded companion diagnostics; never save raw serial/provisioning data.

Reads an existing monitor log, or stops input and snapshots a serial-connected
device. It does not flash, provision, arm input, or claim hardware qualification.
Opening a serial adapter may reset the device: use --minimum-window-seconds to
reject an interval lost to reset. Direct USB capture should use Cockpit's already
open port and --log, rather than competing for that port.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import time
import uuid

METRICS = {"lcd-spi", "touch-spi", "lcd-write", "lcd-bus-wait", "touch-bus-wait", "touch-poll",
           "touch-poll-interval", "render", "present", "host-request", "source-ack", "input-source-ack"}
SUMMARY_FIELDS = {"status", "version", "requestId", "boardProfile", "firmwareVersion", "sampledAtUptimeMs", "windowStartedUptimeMs",
    "serialRequestTxBytes", "serialRequestRxBytes", "wsTxPayloadBytes", "wsRxPayloadBytes", "memorySamples", "freeInternalBytes",
    "minimumInternalBytes", "largestInternalBytes", "smallestLargestInternalBytes", "stackMinBytes", "stackSamples", "queueMax",
    "pendingInputMaxUs", "touchAgeMaxUs", "sourceAckAgeMaxUs", "renderAgeMaxUs"}
METRIC_FIELDS = {"status", "version", "requestId", "metric", "count", "failed", "dropped", "bytes", "totalUs", "maxUs", "p50UpperUs", "p95UpperUs", "p99UpperUs"}

class Capture:
    def __init__(self, request_id=None):
        self.request_id = request_id
        self.summary = None
        self.metrics = {}
        self.complete = False

    def feed(self, raw):
        # Diagnostics have bounded ASCII fields and records below 2 KiB. Drop
        # unrelated lines, including credential-bearing setup/status records.
        if len(raw) > 2048:
            return
        try:
            value = json.loads(raw)
        except (ValueError, UnicodeError):
            return
        if not isinstance(value, dict) or value.get("version") != 1:
            return
        status = value.get("status")
        if status not in ("diagnostics-summary", "diagnostics-metric", "diagnostics-complete"):
            return
        rid = value.get("requestId")
        if not isinstance(rid, str) or not re.fullmatch(r"(?:[A-Za-z0-9][A-Za-z0-9_-]{0,63})?", rid):
            raise ValueError("Invalid diagnostics correlation")
        if self.request_id is not None and rid != self.request_id:
            return
        if status == "diagnostics-summary":
            if not SUMMARY_FIELDS <= value.keys():
                raise ValueError("Incomplete diagnostics summary")
            for field in SUMMARY_FIELDS - {"status", "version", "requestId", "boardProfile", "firmwareVersion", "stackMinBytes", "stackSamples"}:
                self._counter(value[field])
            for field in ("stackMinBytes", "stackSamples"):
                if not isinstance(value[field], list) or len(value[field]) != 3:
                    raise ValueError("Invalid stack sample array")
                for item in value[field]:
                    self._counter(item)
            for field in ("boardProfile", "firmwareVersion"):
                if not isinstance(value[field], str) or not re.fullmatch(r"[A-Za-z0-9._-]{1,80}", value[field]):
                    raise ValueError("Invalid diagnostics identity")
            self.summary = {key: value[key] for key in SUMMARY_FIELDS}
            self.metrics = {}
            self.complete = False
        elif self.summary is not None and rid == self.summary["requestId"]:
            if status == "diagnostics-metric":
                metric = value.get("metric")
                if not METRIC_FIELDS <= value.keys() or metric not in METRICS or metric in self.metrics:
                    raise ValueError("Invalid or duplicate diagnostic metric")
                for field in METRIC_FIELDS - {"status", "version", "requestId", "metric"}:
                    self._counter(value[field])
                if value["failed"] > value["count"]:
                    raise ValueError("Invalid diagnostic failure count")
                self.metrics[metric] = {key: value[key] for key in METRIC_FIELDS}
            elif set(self.metrics) == METRICS:
                self.complete = True
            else:
                raise ValueError("Missing diagnostic metrics")

    @staticmethod
    def _counter(value):
        if type(value) is not int or not 0 <= value <= 18446744073709551615:
            raise ValueError("Invalid diagnostic counter")

    def result(self, minimum_window_seconds=0):
        if not self.complete:
            raise ValueError("No complete diagnostics snapshot")
        window = self.summary["sampledAtUptimeMs"] - self.summary["windowStartedUptimeMs"]
        if window < minimum_window_seconds * 1000:
            raise ValueError("Diagnostics interval is too short or the device reset")
        return {"summary": self.summary, "metrics": self.metrics, "windowMs": window,
                "percentiles": "powers-of-two conservative upper bounds; maxUs is exact",
                "limitsUs": {"pendingInput": 250000, "touchAndRender": 600000, "sourceAck": 1000000}}

def capture_serial(port, baud, timeout, reset):
    import serial
    from provision import encode, request_status, run_command
    connection = serial.Serial(baudrate=baud, timeout=0.25, write_timeout=3)
    connection.dtr = False
    connection.rts = False
    connection.port = port
    connection.open()
    try:
        request_status(connection)  # Let a resetting USB bridge boot first.
        run_command(connection, {"command": "stop"}, timeout=timeout)
        rid = "baseline-" + uuid.uuid4().hex
        connection.write(encode({"command": "diagnostics", "requestId": rid, "reset": reset}))
        connection.flush()
        capture = Capture(rid)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            raw = connection.readline(2049)
            if reset:
                try:
                    value = json.loads(raw)
                except (ValueError, UnicodeError):
                    continue
                if isinstance(value, dict) and value.get("status") == "diagnostics-reset":
                    return None
            else:
                capture.feed(raw)
                if capture.complete:
                    return capture
        raise TimeoutError("No complete diagnostics response")
    finally:
        connection.close()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--log", type=Path, help="Read diagnostics records from a monitor/host log; raw content is never copied")
    source.add_argument("--port", help="Stop and snapshot a serial device; Cockpit/monitor must have released this port")
    parser.add_argument("--baud", type=int, choices=(115200, 921600), default=115200)
    parser.add_argument("--reset", action="store_true", help="Clear counters on a stopped serial device, without flashing or erasing setup")
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--minimum-window-seconds", type=int, default=0)
    parser.add_argument("--request-id", help="Select a correlated diagnostics snapshot in a log")
    parser.add_argument("--firmware", type=Path, help="Hash the tested binary supplied by the operator; capture alone does not verify flashed bytes")
    parser.add_argument("--output", type=Path, help="JSON evidence file")
    args = parser.parse_args()
    if args.minimum_window_seconds < 0 or args.timeout <= 0 or (args.reset and not args.port) or (not args.reset and not args.output):
        parser.error("Provide an output and nonnegative interval, or --port with --reset")
    if args.port:
        capture = capture_serial(args.port, args.baud, args.timeout, args.reset)
        if capture is None:
            print("Diagnostics counters reset; input remains stopped.")
            return
    else:
        capture = Capture(args.request_id)
        with args.log.open("rb") as stream:
            for raw in stream:
                capture.feed(raw)
    result = capture.result(args.minimum_window_seconds)
    result["capturedAtUtc"] = datetime.now(timezone.utc).isoformat()
    result["captureSource"] = "serial-device" if args.port else "operator-supplied-log"
    result["firmwareBinarySha256"] = "sha256:" + hashlib.sha256(args.firmware.read_bytes()).hexdigest() if args.firmware else None
    result["firmwareBinaryVerifiedOnDevice"] = False
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"Saved {len(capture.metrics)} metrics over {result['windowMs']} ms to {args.output}")

if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, TimeoutError, OSError, ImportError):
        # Never echo raw serial lines or exception text containing setup data.
        raise SystemExit("Baseline capture failed. Check port ownership, diagnostics support, complete records and the required interval.")
