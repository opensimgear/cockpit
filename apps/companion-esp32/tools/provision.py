"""Send provisioning records over a trusted serial cable; never print secrets."""
import argparse
import getpass
import json
import re
import time
import uuid
from pathlib import Path

def encode(record):
    data = json.dumps(record, ensure_ascii=True, separators=(",", ":")).encode("ascii")
    if len(data) > 8192:
        raise ValueError("Provisioning record exceeds device budget")
    return data + b"\n"

def status_line(line):
    try:
        value = json.loads(line)
    except (ValueError, UnicodeError):
        return None
    status = value.get("status") if isinstance(value, dict) else None
    return status if isinstance(status, str) and re.fullmatch(r"[a-z-]{1,96}", status) else None

def arm_record(value):
    if not isinstance(value, dict) or value.get("status") != "live-disarmed" or value.get("readyForArm") is not True or value.get("touchAvailable") is not True:
        raise ValueError("A released, healthy touch device and current displayed panel are required")
    result = {"command": "arm"}
    for key in ("sessionId", "generation", "publicationHash", "armEpoch"):
        item = value.get(key)
        if not isinstance(item, str):
            raise ValueError("Missing current arming identity")
        if key == "sessionId":
            valid = re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", item)
        elif key == "publicationHash":
            valid = re.fullmatch(r"sha256:[a-f0-9]{64}", item)
        else:
            valid = re.fullmatch(r"[1-9][0-9]{0,19}", item) and int(item) <= 18446744073709551615
        if not valid:
            raise ValueError("Invalid current arming identity")
        result[key] = item
    return result

def request_status(port, timeout=5):
    request_id = uuid.uuid4().hex
    port.write(encode({"command": "status", "requestId": request_id}))
    port.flush()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        raw = port.readline(8193)
        if len(raw) > 8192 or not status_line(raw):
            continue
        value = json.loads(raw)
        if value.get("requestId") == request_id:
            return value
    raise TimeoutError("No current device status")

def terminal_status(command, status):
    expected = {"provision": "live-disarmed", "connect": "live-disarmed", "resume": "live-disarmed", "arm": "live-armed", "disarm": "disarmed-reconnect-required", "stop": "stopped", "forget": "forgotten", "page": "page-changed"}
    return expected.get(command) == status

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("port", help="Serial port, e.g. COM5 or /dev/ttyUSB0")
    parser.add_argument("--pairing", type=Path, help="Cockpit pairing JSON copied through a trusted path")
    parser.add_argument("--name", default="ESP32 screen")
    parser.add_argument("--instance", default="")
    parser.add_argument("--persist", action="store_true", help="Require protected persistent storage; unavailable boards reject it")
    parser.add_argument("--command", choices=["provision", "resume", "connect", "stop", "forget", "status", "page", "arm", "disarm"], default="provision")
    parser.epilog = "arm explicitly enables live touch controls after a neutral baseline. Connection alone stays disarmed."
    parser.add_argument("--page", type=int, default=0)
    args = parser.parse_args()
    record = {"command": args.command}
    if args.command == "provision":
        record.update(ssid=input("Wi-Fi SSID: "), password=getpass.getpass("Wi-Fi password (empty for open network): "), name=args.name, instanceId=args.instance, persist=args.persist)
        # Obtain the short-lived offer after entering Wi-Fi details.
        raw = args.pairing.read_text(encoding="utf-8") if args.pairing else getpass.getpass("Fresh Cockpit pairing JSON (hidden): ")
        record["pairing"] = json.loads(raw)
    if args.command in ("provision", "resume"):
        record["unixMs"] = int(time.time() * 1000)
    if args.command == "connect":
        record["instanceId"] = args.instance
    if args.command == "page":
        record["page"] = args.page
    import serial
    port = serial.Serial(baudrate=115200, timeout=0.25, write_timeout=3)
    port.dtr = False
    port.rts = False
    port.port = args.port
    port.open()
    try:
        if args.command == "status":
            print(request_status(port)["status"])
            return
        if args.command == "arm":
            record = arm_record(request_status(port))
        port.write(encode(record))
        port.flush()
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            status = status_line(port.readline(8193))
            if status:
                print(status)
                if terminal_status(args.command, status):
                    return
                if any(word in status for word in ("failed", "invalid", "rejected", "unavailable", "expired", "unsupported", "required")):
                    raise RuntimeError("Device did not complete the requested operation")
        raise TimeoutError("Device response timed out; inspect its status before retrying")
    finally:
        port.close()

if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, TimeoutError, OSError):
        # Do not include exception text that may echo sensitive input.
        raise SystemExit("Provisioning did not complete. Check the port, device status and fresh pairing offer.")
