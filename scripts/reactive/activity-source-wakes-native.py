#!/usr/bin/env python3
"""Hold one authenticated remote companion socket for native Activity wake acceptance."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "companion_mapping_acceptance", ROOT / "scripts/companion-mapping-acceptance.py"
)
companion = importlib.util.module_from_spec(spec)
spec.loader.exec_module(companion)


class NativeCockpit:
    def __init__(self, endpoint):
        self.endpoint = endpoint

    def request(self, command, **fields):
        import urllib.request

        request = urllib.request.Request(
            self.endpoint,
            json.dumps({"command": command, **fields}).encode("utf-8"),
            {
                "Content-Type": "application/json",
                "Authorization": "Bearer " + os.environ["OSG_ACTIVITY_SOURCE_BRIDGE_TOKEN"],
            },
        )
        with urllib.request.urlopen(request, timeout=20) as response:
            result = response.read(64 * 1024 + 1)
        if len(result) > 64 * 1024:
            raise RuntimeError("Native pairing bridge response exceeded its limit")
        return json.loads(result)


def run():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bridge", required=True)
    parser.add_argument("--companion-id", required=True)
    args = parser.parse_args()
    service = NativeCockpit(args.bridge)
    pairing = json.loads(
        companion.operator(service, "companion_pair_offer", kind="desktop")["pairing"]["manualPayload"]
    )
    socket = companion.Socket(pairing)
    try:
        paired = socket.request(
            {"type": "pair", "secret": pairing["secret"], "companionId": args.companion_id, "name": args.companion_id}
        )
        if not paired.get("ok"):
            raise RuntimeError("Native host rejected the test companion pairing")
        companion.operator(
            service,
            "companion_approve",
            requestId=paired["requestId"],
            companionId=args.companion_id,
        )
        status = socket.request(
            {"type": "pair-status", "requestId": paired["requestId"], "secret": pairing["secret"]}
        )
        if not status.get("ok") or status.get("companionId") != args.companion_id:
            raise RuntimeError("Native host did not confirm the test companion pairing")
        authenticated = socket.request({"type": "auth", "token": status["token"]})
        if not authenticated.get("ok"):
            raise RuntimeError("Native host rejected test companion authentication")
        print(json.dumps({"type": "ready", "companionId": args.companion_id}), flush=True)
        sys.stdin.readline()
    finally:
        socket.close()


if __name__ == "__main__":
    try:
        run()
    except Exception as error:
        # Keep pairing secrets, bearer credentials and certificate material out of logs.
        print(json.dumps({"type": "error", "error": type(error).__name__}), flush=True)
        raise SystemExit(1)
