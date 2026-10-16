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

def request_status(port, timeout=8):
    request_id = uuid.uuid4().hex
    wire = encode({"command": "status", "requestId": request_id})
    deadline = time.monotonic() + timeout
    next_probe = 0
    # Only this harmless, correlated status probe may be retried. USB-UART
    # control-line changes can reset the board as the port opens.
    while time.monotonic() < deadline:
        now = time.monotonic()
        if now >= next_probe:
            port.write(wire)
            port.flush()
            next_probe = now + 0.5
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

def run_command(port, record, timeout=90):
    command = record["command"]
    current = request_status(port)
    if command == "status":
        print(current["status"])
        return
    if command == "arm":
        record = arm_record(current)
    # Provisioning, baseline arming and other mutations are sent exactly once.
    port.write(encode(record))
    port.flush()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        status = status_line(port.readline(8193))
        if status:
            print(status)
            if terminal_status(command, status):
                return
            if any(word in status for word in ("failed", "invalid", "rejected", "unavailable", "expired", "unsupported", "required")):
                raise RuntimeError("Device did not complete the requested operation")
    raise TimeoutError("Device response timed out; inspect status before retrying")

def console_record(raw, instance=""):
    parts = raw.strip().split()
    if not parts:
        return None
    command = parts[0].removeprefix("screen_")
    if command == "page" and len(parts) == 2 and parts[1].isdigit() and 0 <= int(parts[1]) < 4:
        return {"command": "page", "page": int(parts[1])}
    if len(parts) != 1 or command not in ("status", "arm", "disarm", "connect", "stop", "quit"):
        raise ValueError("Unsupported console command")
    if command == "quit":
        return {"command": "stop"}
    if command == "connect":
        return {"command": command, "instanceId": instance}
    return {"command": command}

def interactive_console(port, instance=""):
    print("Serial connection stays open. Commands: status, arm, disarm, connect, page N, stop, quit.")
    print("Tap and release once before arm. Use Cockpit recording mode for initial touch tests.")
    try:
        while True:
            raw = input("Device command> ")
            try:
                record = console_record(raw, instance)
            except ValueError:
                print("Use status, arm, disarm, connect, page N, stop or quit.")
                continue
            if record is None:
                continue
            quitting = raw.strip().removeprefix("screen_") == "quit"
            try:
                run_command(port, record)
            except (ValueError, RuntimeError, TimeoutError, OSError):
                print("Command did not complete. Check status; reconnect explicitly if required.")
            if quitting:
                return
    except (EOFError, KeyboardInterrupt):
        # Explicit console exit cancels the source before releasing the port.
        try:
            run_command(port, {"command": "stop"}, timeout=3)
        except (ValueError, RuntimeError, TimeoutError, OSError):
            pass
        print("Console closed.")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("port", help="Serial port, e.g. COM5 or /dev/ttyUSB0")
    parser.add_argument("--pairing", type=Path, help="Cockpit pairing JSON copied through a trusted path")
    parser.add_argument("--name", default="ESP32 screen")
    parser.add_argument("--instance", default="")
    parser.add_argument("--persist", action="store_true", default=True, help="Persist setup on the board (default)")
    parser.add_argument("--no-persist", dest="persist", action="store_false", help="Use temporary setup until power loss")
    parser.add_argument("--command", choices=["provision", "resume", "connect", "stop", "forget", "status", "page", "arm", "disarm"], default="provision")
    parser.epilog = "arm explicitly enables live touch controls after a neutral baseline. Connection alone stays disarmed."
    parser.add_argument("--interactive", action="store_true", help="Keep one serial connection open for provisioning and live controls")
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
    port = serial.Serial(baudrate=921600, timeout=0.25, write_timeout=3)
    port.dtr = False
    port.rts = False
    port.port = args.port
    port.open()
    try:
        run_command(port, record)
        record.clear()
        if args.interactive:
            raw = None
            interactive_console(port, args.instance)
    finally:
        port.close()

if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, TimeoutError, OSError):
        # Do not include exception text that may echo sensitive input.
        raise SystemExit("Provisioning did not complete. Check the port, device status and fresh pairing offer.")
