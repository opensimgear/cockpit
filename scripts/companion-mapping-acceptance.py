#!/usr/bin/env python3
"""Exercise real isolated Gateway -> WSS peers -> companion recording routes. No virtual output."""
import argparse
import base64
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import socket
import ssl
import struct
import tempfile
import threading
import time
import uuid
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("mapping_acceptance", ROOT / "scripts/mapping-acceptance.py")
mapping = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mapping)
FRAME_BYTES = 16 * 1024
TRANSFER_BYTES = 13 * 1024 * 1024
PROTOCOL = "opensimgear-companion-v1"


def encoded_frame(value, opcode=1):
    payload = value if isinstance(value, bytes) else json.dumps(value, separators=(",", ":")).encode()
    assert len(payload) <= FRAME_BYTES, "Acceptance peer exceeded frame budget"
    mask = os.urandom(4)
    size = len(payload)
    prefix = bytes([0x80 | opcode, 0x80 | size]) if size < 126 else bytes([0x80 | opcode, 0x80 | 126]) + struct.pack("!H", size)
    return prefix + mask + bytes(value ^ mask[index % 4] for index, value in enumerate(payload))


class Socket:
    def __init__(self, pairing):
        address = urlsplit(pairing["endpoint"])
        assert address.scheme == "wss" and address.hostname == "127.0.0.1" and address.path == "/companion" and not address.query
        # Exact CA arrives over isolated private operator IPC; this simulated peer
        # is not evidence for the native client's separate out-of-band SPKI check.
        certificate = ssl.DER_cert_to_PEM_cert(base64.b64decode(pairing["caCertificate"], validate=True))
        context = ssl.create_default_context(cadata=certificate)
        self.socket = context.wrap_socket(socket.create_connection((address.hostname, address.port), timeout=3), server_hostname=address.hostname)
        self.buffer = bytearray()
        key = base64.b64encode(os.urandom(16)).decode()
        self.socket.sendall((f"GET /companion HTTP/1.1\r\nHost: {address.netloc}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: {PROTOCOL}\r\n\r\n").encode())
        while b"\r\n\r\n" not in self.buffer:
            chunk = self.socket.recv(4096)
            if not chunk:
                raise RuntimeError("WSS upgrade closed unexpectedly")
            self.buffer.extend(chunk)
            assert len(self.buffer) <= 32768, "Unbounded handshake"
        header, tail = self.buffer.split(b"\r\n\r\n", 1)
        self.buffer = bytearray(tail)
        lines = header.decode("ascii").split("\r\n")
        fields = {name.lower(): value.strip() for name, value in (line.split(":", 1) for line in lines[1:])}
        expected = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        assert lines[0].startswith("HTTP/1.1 101 ") and fields["sec-websocket-accept"] == expected and fields["sec-websocket-protocol"] == PROTOCOL

    def exact(self, size):
        while len(self.buffer) < size:
            chunk = self.socket.recv(min(8192, size - len(self.buffer)))
            if not chunk:
                raise RuntimeError("WSS peer closed unexpectedly")
            self.buffer.extend(chunk)
        result = bytes(self.buffer[:size])
        del self.buffer[:size]
        return result

    def frame(self):
        first, second = self.exact(2)
        assert first & 0x80 and not first & 0x70 and not second & 0x80, "Unexpected fragmented/masked server frame"
        size = second & 0x7f
        if size == 126:
            size = struct.unpack("!H", self.exact(2))[0]
        elif size == 127:
            size = struct.unpack("!Q", self.exact(8))[0]
        assert size <= FRAME_BYTES, "Server frame exceeded budget"
        payload = self.exact(size)
        opcode = first & 15
        if opcode == 9:
            self.socket.sendall(encoded_frame(payload, 10))
            return self.frame()
        assert opcode == 1, "Expected bounded JSON text frame"
        return json.loads(payload)

    def request(self, value):
        self.socket.sendall(encoded_frame(value))
        result = self.frame()
        if result.get("type") != "transfer-start":
            return result
        assert 0 < result["bytes"] <= TRANSFER_BYTES
        transferred = bytearray()
        index = 0
        while True:
            chunk = self.frame()
            assert chunk["id"] == result["id"]
            if chunk["type"] == "transfer-end":
                break
            assert chunk["type"] == "transfer-chunk" and chunk["index"] == index
            transferred.extend(base64.b64decode(chunk["base64"], validate=True))
            assert len(transferred) <= result["bytes"]
            index += 1
        assert len(transferred) == result["bytes"] and "sha256:" + hashlib.sha256(transferred).hexdigest() == result["sha256"]
        return json.loads(transferred)

    def close(self):
        self.socket.close()


class Peer:
    def __init__(self, service, name):
        self.name = name
        pairing = json.loads(operator(service, "companion_pair_offer")["pairing"]["manualPayload"])
        self.socket = Socket(pairing)
        reply = self.socket.request(dict(type="pair", secret=pairing["secret"], companionId=name, name=name))
        assert reply["ok"], reply
        operator(service, "companion_approve", requestId=reply["requestId"], companionId=name)
        paired = self.socket.request(dict(type="pair-status", requestId=reply["requestId"], secret=pairing["secret"]))
        assert paired["ok"] and paired["hostId"] == pairing["hostId"] and paired["companionId"] == name
        assert self.socket.request(dict(type="auth", token=paired["token"]))["ok"]
        self.lock = threading.Lock()
        self.stopped = threading.Event()
        self.paused = threading.Event()
        self.failure = None
        self.opened = None
        self.sequence = 0
        self.heartbeat = 0
        self.thread = threading.Thread(target=self.renew, daemon=True)
        self.thread.start()

    def renew(self):
        while not self.stopped.wait(0.2):
            if self.paused.is_set():
                continue
            with self.lock:
                if self.opened is None:
                    continue
                self.heartbeat += 1
                try:
                    result = self.socket.request(self.message("heartbeat", sequence=str(self.heartbeat)))
                    assert result["ok"], result
                except Exception as error:
                    self.failure = error
                    return

    def message(self, kind, **fields):
        return dict(version=1, type=kind, **{key: self.opened[key] for key in ["clientId", "sessionId", "generation", "publicationHash"]}, **fields)

    def open(self, instance, publication):
        with self.lock:
            result = self.socket.request(dict(type="open", instanceId=instance, publicationHash=publication["publicationHash"]))
            assert result["ok"], result
            self.opened = result
            self.sequence = self.heartbeat = 0
            controls = json.loads(publication["canonicalDocument"])["ui"]["controls"]
            values = {control["id"]: False if control["kind"] == "button" else control.get("fallback", -1) for control in controls}
            assert self.socket.request(self.message("baseline", sequence="0", values=values))["ok"]

    def input(self, changes):
        with self.lock:
            assert self.failure is None, self.failure
            self.sequence += 1
            result = self.socket.request(self.message("input", sequence=str(self.sequence), changes=changes))
            assert result["ok"], result

    def end(self):
        with self.lock:
            if self.opened:
                result = self.socket.request(self.message("close"))
                assert result["ok"], result
                self.opened = None

    def close(self):
        self.stopped.set()
        self.thread.join(timeout=4)
        self.socket.close()


def operator(service, command, **payload):
    reply = service.request(command, companion=payload)
    assert reply["ok"], reply
    return reply["state"]["companions"]


def route(service, instance, action="inspect", **payload):
    deadline = time.monotonic() + 4
    while True:
        reply = service.request("mapping_companion_route", companion=dict(action=action, instanceId=instance, **payload))
        if reply["ok"]:
            return reply["state"]["mappings"]["companionRoute"]
        # Native I/O is intentionally not awaited by recording-page requests.
        # Retry only this explicit transient response; preserve all other faults.
        if action != "recording" or not reply.get("error", "").startswith("This companion's feeder is busy;") or time.monotonic() >= deadline:
            raise AssertionError(reply)
        time.sleep(0.02)


def publish(service, publication):
    payload = json.dumps({key: publication[key] for key in ["canonicalDocument", "publicationHash", "assets"]}, separators=(",", ":")).encode()
    upload = operator(service, "companion_publish_begin", size=len(payload))["uploadId"]
    for offset in range(0, len(payload), 8192):
        operator(service, "companion_publish_chunk", uploadId=upload, offset=offset, bytes=list(payload[offset:offset + 8192]))
    operator(service, "companion_publish_commit", uploadId=upload)


def wait_for(predicate, message):
    deadline = time.monotonic() + 4
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.02)
    raise AssertionError(message)


def sample(publication):
    document = json.loads(publication["canonicalDocument"])
    controls = {kind: next((control for control in document["ui"]["controls"] if control["kind"] == kind), None) for kind in ["button", "axis", "hat"]}
    return [dict(controlId=controls[kind]["id"], value=value) for kind, value in [("button", True), ("axis", 0.75), ("hat", 7)] if controls[kind]]


def latest_states(service, instance):
    page = route(service, instance, "recording", after="0")
    result = {}
    for record in page.get("records", []):
        result[record["report"]["target"]] = record["report"]["state"]
    return result


def fallback_states(profile):
    return {target["id"]: dict(buttons=[False] * target["layout"]["buttons"], axes=[axis["fallback"] for axis in target["layout"]["axes"]], hats=[None] * target["layout"]["hats"]) for target in profile["targets"]}


def axis_value(value):
    scaled = value * (32768 if value < 0 else 32767)
    return math.ceil(scaled - 0.5) if scaled < 0 else math.floor(scaled + 0.5)


def expected_sample(profile, publication):
    document = json.loads(publication["canonicalDocument"])
    targets = dict(zip((target["id"] for target in document["targets"]), profile["targets"]))
    result = fallback_states(profile)
    for control in document["ui"]["controls"]:
        if control["kind"] != "axis":
            continue
        binding = next(binding for binding in document["bindings"] if binding["controlId"] == control["id"])
        assert targets[binding["targetId"]]["layout"]["axes"][binding["index"]]["fallback"] == axis_value(control["fallback"]), "Authored route discarded a declared axis fallback"
    for change in sample(publication):
        binding = next(binding for binding in document["bindings"] if binding["controlId"] == change["controlId"])
        target = targets[binding["targetId"]]
        state = result[target["id"]]
        field = {"button": "buttons", "axis": "axes", "hat": "hats"}[binding["kind"]]
        state[field][binding["index"]] = axis_value(change["value"]) if binding["kind"] == "axis" else change["value"]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--service", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    flight = json.loads((ROOT / "crates/companion-service/tests/fixtures/c1-flight-publication.json").read_text())
    racing = json.loads((ROOT / "scripts/fixtures/companion-racing-publication.json").read_text())
    evidence = dict(qualification=False, virtualReportsSent=False, driverLayoutsChanged=False,
                    peers="simulated real WSS on loopback", nativeClientPinQualification=False,
                    physicalGameQualification=False, checks=[])
    with tempfile.TemporaryDirectory(prefix="osg-companion-mapping-acceptance-") as temporary:
        directory = Path(temporary)
        service = mapping.Service(args.service.resolve(), directory)
        peers = []
        try:
            initial = service.mapping("mapping_status")
            assert not initial["virtualDevices"]["qualified"] and not initial["virtualDevices"]["enabled"]
            ordinary = json.loads((ROOT / "crates/mapping/fixtures/direct-profile.json").read_text())
            ordinary["id"] = str(uuid.uuid4())
            mapping.upload(service, ordinary)
            service.mapping("mapping_assignments_save", profileId=ordinary["id"], assignments=mapping.assignments(ordinary))
            ordinary_token = service.mapping("mapping_preview_start", profileId=ordinary["id"])["token"]
            with socket.socket() as probe:
                probe.bind(("127.0.0.1", 0))
                port = probe.getsockname()[1]
            operator(service, "companion_enable", address="127.0.0.1", advertisedHost="127.0.0.1", port=port, discovery=False)
            publish(service, flight)
            publish(service, racing)
            configurations = [("flight-left", flight), ("flight-right", flight), ("racing", racing)]
            profiles = {}
            for instance, publication in configurations:
                peer = Peer(service, instance)
                peers.append(peer)
                operator(service, "companion_assign", companionId=instance, instanceId=instance, publicationHash=publication["publicationHash"])
                inspected = route(service, instance)
                assert not inspected["preflight"]["liveReady"]
                profiles[instance] = inspected["profile"]
                route(service, instance, "save", publicationHash=publication["publicationHash"], bindings=[])
                peer.open(instance, publication)
                route(service, instance, "start", mode="recording")
            assert len({profile["id"] for profile in profiles.values()}) == 3
            assert len({target["id"] for profile in profiles.values() for target in profile["targets"]}) == sum(len(profile["targets"]) for profile in profiles.values())
            evidence["checks"].append("three independent authored routes, including two same-UI instances")
            for peer, (_, publication) in zip(peers, configurations):
                peer.input(sample(publication))
            active = {instance: expected_sample(profiles[instance], publication) for instance, publication in configurations}
            for instance, _ in configurations:
                wait_for(lambda i=instance: latest_states(service, i) == active[i], f"{instance} did not record exact axes/buttons/hats")
            evidence["checks"].append("exact simultaneous axis/button/hat state and each declared axis fallback")
            assert service.mapping("mapping_status")["running"]
            route(service, "flight-left", "stop")
            wait_for(lambda: latest_states(service, "flight-left") == fallback_states(profiles["flight-left"]), "Instance Stop did not apply declared fallback")
            assert latest_states(service, "flight-right") == active["flight-right"] and latest_states(service, "racing") == active["racing"]
            evidence["checks"].append("per-instance Stop preserves other routes and ordinary recording")
            route(service, "flight-left", "start", mode="recording")
            peers[0].input(sample(flight))
            wait_for(lambda: latest_states(service, "flight-left") == active["flight-left"], "Explicit recording restart did not resume the chosen route")
            operator(service, "companion_revoke", companionId="flight-left")
            wait_for(lambda: latest_states(service, "flight-left") == fallback_states(profiles["flight-left"]), "Revocation did not neutralize its route")
            assert latest_states(service, "flight-right") == active["flight-right"] and latest_states(service, "racing") == active["racing"]
            evidence["checks"].append("revocation releases only the affected route")
            peers[1].end()
            wait_for(lambda: latest_states(service, "flight-right") == fallback_states(profiles["flight-right"]), "Source Stop did not apply fallback")
            assert latest_states(service, "racing") == active["racing"]
            evidence["checks"].append("wire source Stop remains isolated")
            wait_for(lambda: route(service, "flight-right")["session"]["cleanupComplete"], "Source loss cleanup did not finish")
            peers[1].open("flight-right", flight)
            peers[1].input(sample(flight))
            for _ in range(3):
                assert latest_states(service, "flight-right") == fallback_states(profiles["flight-right"]), "Reconnect silently revived a stopped mapping"
                assert not route(service, "flight-right")["session"]["running"]
                time.sleep(0.05)
            route(service, "flight-right", "start", mode="recording")
            wait_for(lambda: latest_states(service, "flight-right") == active["flight-right"], "Fresh explicit Start did not accept the new source baseline")
            peers[1].end()
            wait_for(lambda: latest_states(service, "flight-right") == fallback_states(profiles["flight-right"]), "Repeated source Stop did not apply fallback")
            evidence["checks"].append("source reconnect stays disarmed until fresh explicit mapping Start")
            peers[2].paused.set()
            wait_for(lambda: latest_states(service, "racing") == fallback_states(profiles["racing"]), "Native source lease failed to expire")
            assert service.mapping("mapping_status")["running"]
            service.mapping("mapping_simulate", token=ordinary_token, source=mapping.assignments(ordinary)[0]["source"], changes=[dict(control="trigger", value=dict(kind="button", value=True))])
            evidence["checks"].append("source lease loss neutralizes affected route while ordinary mapping stays active")
            rejected = service.request("mapping_companion_route", companion=dict(action="start", instanceId="flight-right", mode="live"))
            assert not rejected["ok"], "Unqualified output must not activate"
            final = service.mapping("mapping_status")
            assert not final["virtualDevices"]["qualified"] and not final["virtualDevices"]["enabled"]
            evidence["checks"].append("unqualified live Start rejected; no output enablement or provisioning")
            for peer in peers:
                peer.close()
            peers.clear()
            operator(service, "companion_disable")
        finally:
            for peer in peers:
                peer.close()
            service.close()
        service = mapping.Service(args.service.resolve(), directory)
        try:
            state = operator(service, "companion_status")
            assert not state["listener"]["enabled"] and all(not item["armed"] for item in state["host"]["instances"])
            assert not service.mapping("mapping_status")["running"]
            assert {item["instanceId"] for item in state["host"]["instances"]} == {"flight-right", "racing"}
            publish(service, flight)
            publish(service, racing)
            for instance in ["flight-right", "racing"]:
                retained = route(service, instance)
                assert retained["profile"]["id"] == profiles[instance]["id"] and not retained["session"]["running"]
            evidence["checks"].append("restart retains intended offline descriptors without activating listener or mapping")
        finally:
            service.close()
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence, indent=2))


if __name__ == "__main__":
    main()
