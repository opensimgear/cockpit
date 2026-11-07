#!/usr/bin/env python3
"""Produce a timed encoder burst from an authenticated companion WSS peer."""
import argparse
import importlib.util
import json
import socket
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


companion = load("encoder_companion", "companion-mapping-acceptance.py")
native = load("encoder_native", "desktop-companion-native.py")


def emit(value):
    sys.stdout.write(json.dumps(value, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def read_command():
    line = sys.stdin.readline()
    if not line:
        return None
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        return line.strip()


def wait_until(target_ns):
    while True:
        remaining = target_ns - time.perf_counter_ns()
        if remaining <= 0:
            return
        time.sleep(min(remaining / 1_000_000_000, 0.001))


def queue_burst(peer, start_ns, detents):
    assert 1 <= detents <= 24, "detents must be between 1 and 24"
    offered = []
    period_ns = 8_000_000
    down_ns = 4_000_000
    # Keep the regular heartbeat worker running while waiting for the common
    # start. One immediate renewal covers the bounded burst and ACK drain.
    wait_until(start_ns - 10_000_000)
    with peer.lock:
        assert peer.failure is None, peer.failure
        peer.heartbeat += 1
        reply = peer.socket.request(peer.message('heartbeat', sequence=str(peer.heartbeat)))
        assert reply.get('ok') is True, reply
        peer.socket.socket.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        for index in range(detents):
            for value, offset in ((True, 0), (False, down_ns)):
                target_ns = start_ns + index * period_ns + offset
                wait_until(target_ns)
                at_ns = time.perf_counter_ns()
                peer.sequence += 1
                sequence = peer.sequence
                frame = companion.encoded_frame(peer.message(
                    "input", sequence=str(sequence),
                    changes=[dict(controlId="flight-input-5", value=value)]))
                offered.append(dict(sequence=sequence, value=value, atNs=str(at_ns)))
                peer.socket.socket.sendall(frame)

        # Read acknowledgements only after all edge frames have been offered.
        for expected in offered:
            reply = peer.socket.frame()
            assert reply.get("ok") is True, reply
            assert str(reply.get("sequence")) == str(expected["sequence"]), reply
    return offered


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bridge", required=True)
    parser.add_argument("--instance", required=True)
    args = parser.parse_args()
    service = native.NativeCockpit(args.bridge)
    peer = None
    try:
        peer = companion.Peer(service, args.instance)
        emit(dict(phase="paired"))

        command = read_command()
        if command is None or command == "close":
            return
        if isinstance(command, dict):
            publication_hash = command.get("publicationHash")
            if not isinstance(publication_hash, str) or not publication_hash:
                raise ValueError("Expected publicationHash")
            publication = json.loads((ROOT / "scripts/fixtures/compiled-flight-publication.json").read_text(encoding="utf-8"))
            if publication["publicationHash"] != publication_hash:
                raise ValueError("Requested publicationHash does not match the fixture")
        elif command == "open":
            publication = json.loads((ROOT / "scripts/fixtures/compiled-flight-publication.json").read_text(encoding="utf-8"))
        else:
            raise ValueError("Expected publicationHash JSON or 'open'")

        peer.open(args.instance, publication)
        ready = peer.ready_session()
        if ready is None:
            raise RuntimeError("Peer did not acknowledge its neutral baseline")
        emit(dict(phase="ready", session=ready))

        command = read_command()
        if command is None or command == "close":
            return
        if not isinstance(command, dict):
            raise ValueError("Expected burst JSON with startNs and detents")
        start_ns = int(command["startNs"])
        detents = int(command["detents"])
        frames = queue_burst(peer, start_ns, detents)
        emit(dict(phase="burst", frames=frames))

        while read_command() not in (None, "close"):
            pass
    except Exception as error:
        emit(dict(phase="error", error=f"{type(error).__name__}: {error}"))
        raise
    finally:
        if peer is not None:
            peer.close()


if __name__ == "__main__":
    main()
