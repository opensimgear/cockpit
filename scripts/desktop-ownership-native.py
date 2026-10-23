#!/usr/bin/env python3
"""Opt-in two real WSS sources sharing MouseLeft in the running native Cockpit.

Only the applied ProfileSession submits OS input. The receiver merely records
marked native messages; this helper supplies authenticated companion traffic.
Publication writes while applied are expected to be refused by native ownership.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import socket
import sys
import threading
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ownership_native_helpers", ROOT / "scripts/desktop-companion-native.py")
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)
companion, mapping = native.companion, native.mapping
LEFT = dict(kind="mouse-button", button="left")
CONTROL = "flight-input-0"


class TransportPeer(companion.Peer):
    """The shared peer protocol with an in-memory credential for reconnect proof.

    The generic acceptance peer does not retain its credential. Keep this local
    rather than changing shared pairing or heartbeat helpers for one native case.
    Credentials and pairing secrets are never included in acceptance evidence.
    """
    def __init__(self, service, name):
        self.name = name
        self._pairing = json.loads(companion.operator(service, "companion_pair_offer")["pairing"]["manualPayload"])
        self.socket = companion.Socket(self._pairing)
        reply = self.socket.request(dict(type="pair", secret=self._pairing["secret"], companionId=name, name=name))
        assert reply["ok"], reply
        companion.operator(service, "companion_approve", requestId=reply["requestId"], companionId=name)
        paired = self.socket.request(dict(type="pair-status", requestId=reply["requestId"], secret=self._pairing["secret"]))
        assert paired["ok"] and paired["hostId"] == self._pairing["hostId"] and paired["companionId"] == name
        self._credential = paired["token"]
        authenticated = self.socket.request(dict(type="auth", token=self._credential))
        assert authenticated["ok"] and authenticated["hostId"] == self._pairing["hostId"] and authenticated["companionId"] == name
        self.lock = threading.Lock()
        self.stopped, self.paused = threading.Event(), threading.Event()
        self.failure = self.opened = self.baseline_session = None
        self.sequence = self.heartbeat = 0
        self.thread = threading.Thread(target=self.renew, daemon=True)
        self.thread.start()

    def abort_transport(self):
        """Shutdown TCP beneath TLS/WSS without any application close message."""
        self.paused.set()
        with self.lock:
            assert self.failure is None and self.opened is not None, self.failure
            transport = self.socket.socket
            result = dict(mode="SSLSocket.shutdown(SHUT_RDWR) followed by socket close",
                protocolSessionCloseSent=False, webSocketCloseFrameSent=False, tlsUnwrapCalled=False,
                localEndpoint=list(transport.getsockname()), remoteEndpoint=list(transport.getpeername()),
                oldSession={key: self.opened[key] for key in ("clientId", "sessionId", "generation", "controllerInstanceId", "publicationHash")})
            self.stopped.set()
            self.opened = self.baseline_session = None
            try:
                transport.shutdown(socket.SHUT_RDWR)
            finally:
                transport.close()
        self.thread.join(timeout=4)
        assert not self.thread.is_alive(), "A's old heartbeat thread survived transport shutdown"
        return result

    def reconnect_transport(self, previous):
        """Authenticate a new pinned-CA WSS connection with the same credential."""
        assert self.stopped.is_set() and not self.thread.is_alive() and self.opened is None
        self.socket = companion.Socket(self._pairing)
        authenticated = self.socket.request(dict(type="auth", token=self._credential))
        assert authenticated["ok"] and authenticated["hostId"] == self._pairing["hostId"] and authenticated["companionId"] == self.name
        transport = self.socket.socket
        local, remote = list(transport.getsockname()), list(transport.getpeername())
        assert local != previous["localEndpoint"] and remote == previous["remoteEndpoint"], "Reconnect did not establish a distinct TCP connection to the same host"
        self.failure = None
        self.stopped.clear()
        self.paused.set()  # The caller must prove the real full-neutral baseline.
        self.thread = threading.Thread(target=self.renew, daemon=True)
        self.thread.start()
        return dict(credentialReused=True, newPairingApproval=False, hostId=authenticated["hostId"],
            companionId=authenticated["companionId"], authenticated=True, localEndpoint=local, remoteEndpoint=remote)


def replacement(publication):
    # Preserve the RFC8785 canonical number spellings in the fixture. Replacing
    # one string value leaves its canonical key order and other bytes intact.
    old = '"title":"flight"'
    canonical = publication["canonicalDocument"]
    assert canonical.count(old) == 1
    canonical = canonical.replace(old, '"title":"Ownership replacement"')
    return dict(canonicalDocument=canonical, publicationHash="sha256:" + hashlib.sha256(canonical.encode()).hexdigest(), assets=publication["assets"])


def profile_for(service, configurations, previous=None):
    document = mapping.blank("Native shared mouse ownership " + uuid.uuid4().hex[:8])
    if previous:
        document.update(id=previous["id"], name=previous["name"], revision=previous["revision"])
    assignments, details = [], {}
    for logical, (instance, publication) in zip(("a", "b"), configurations):
        def current():
            value = companion.selected_resource(service, instance, require_current=False)
            return value if value and all(port["current"] for port in value["ports"]) else None
        detail = mapping.wait_for(current, "No exact current companion ports for " + logical)
        observed = next(port for port in detail["ports"] if port["descriptor"]["id"] == CONTROL)
        assert observed["current"] and observed["descriptor"]["direction"] == "output"
        contract = observed["descriptor"]["contract"]
        assert contract["delivery"] == "state" and contract["schema"] == dict(type="boolean")
        resource = "panel-" + logical
        document["resources"].append(dict(id=resource, name="Real WSS source " + logical.upper(), kind="companion", required=False,
            ports=[mapping.port("button", contract, "output")]))
        document["companionAssignments"].append(dict(resourceId=resource, publicationHash=publication["publicationHash"]))
        document["bindings"].append(dict(id="route-" + logical, **{"from": dict(nodeId=resource, portId="button"), "to": dict(nodeId="desktop", portId="mouse-" + logical)}))
        assignments.append(dict(resourceId=resource, sourceId=detail["resource"]["id"], ports=[dict(profilePortId="button", observedPortId=CONTROL, expectedContractHash=observed["contractHash"])]))
        details[logical] = detail
    target = dict(delivery="state", schema=dict(type="boolean"), freshnessMs=1000, fallback=False)
    document["desktopOutputs"] = [dict(id="desktop", name="Shared native MouseLeft", required=True,
        ports=[mapping.port("mouse-" + logical, target, "input") for logical in ("a", "b")],
        actions=[dict(portId="mouse-" + logical, action=dict(kind="mouse-hold", button="left")) for logical in ("a", "b")])]
    return document, assignments, details


def mouse_edges(path):
    return [event for event in native.injected_events(path) if event["message"] in (513, 514, 515)]


def backend(status):
    result = status["runtime"]["backend"]
    assert not result["uncertain"] and result["inFlight"] is None and result["possibleHeld"] == result["held"], result
    return result


def held_producers(status, pins):
    owners = backend(status)["owners"]
    # An armed producer may remain in the ledger with an empty contribution
    # after release or cancellation. Only nonempty contributions hold a button.
    assert all(owner["controls"] in ([], [LEFT]) for owner in owners), owners
    result = {}
    for owner in owners:
        if not owner["controls"]:
            continue
        action = owner["actionId"]
        assert action in pins["generations"] and action not in result, owner
        producer = owner["producer"]
        logical = "panel-" + action.removeprefix("route-")
        assert producer["run"] == pins["run"] and producer["nodeId"] == logical, producer
        dependencies = producer["dependencies"]
        assert len(dependencies) == 1, dependencies
        dependency = dependencies[0]
        assert dependency["endpoint"] == dict(nodeId=logical, portId="button"), dependency
        assert dependency["generation"] == pins["generations"][action], dependency
        if action in pins["stable"]:
            assert producer == pins["stable"][action], "A source change replaced B's full held producer identity"
        result[action] = producer
    return result


def wait_owned(service, path, owners, pattern, message, pins):
    """Assert no extra release/press during the whole bounded observation wait."""
    expected = set(owners)
    def ready():
        actual = [event["message"] for event in mouse_edges(path)]
        assert actual == pattern[:len(actual)], "Unexpected native MouseLeft edge: " + str(actual)
        status = native.desktop_status(service)
        state = backend(status)
        producers = held_producers(status, pins)
        names = set(producers)
        held = [LEFT] if expected else []
        if names == expected and state["held"] == held and actual == pattern:
            assert state["sharedControls"] == (1 if len(expected) > 1 else 0), state
            return status
        return None
    return mapping.wait_for(ready, message, seconds=8)


def checkpoint(service, peers, configurations):
    return companion.companion_checkpoint(service, configurations, peers)


def producer_pins(owner, sources, instances):
    return dict(run=dict(session=owner["session"], generation=str(owner["generation"])),
        generations={"route-" + logical: str(sources[instance]["generation"]) for logical, instance in zip(("a", "b"), instances)},
        stable={})


def position_receiver(service, path, identity):
    """Obtain a fresh receiver ACK and prove native focus before any mouse hold."""
    def unheld(status):
        if status.get("runtime") is None:
            run = service.profiles("profile_status")["profileSession"]
            assert run["state"] == "stopped" and run["owner"] is None, run
            return
        state = backend(status)
        assert state["held"] == state["possibleHeld"] == [] and state["cleanupComplete"], state
        assert all(not owner["controls"] for owner in state["owners"]), state
    unheld(native.desktop_status(service))
    prior = mapping.wait_for(lambda: report if (report := native.receiver(path)).get("identity") else None, "No complete receiver snapshot before positioning")
    assert prior["identity"] == identity, "The receive-only process identity changed"
    previous_requests = prior.get("focusRequests", 0)
    marker = path.with_suffix(".focus")
    temporary = path.with_suffix(".focus-request.tmp")
    temporary.write_text("focus-and-center")
    temporary.replace(marker)

    def positioned():
        report = native.receiver(path)
        status = native.desktop_status(service)
        unheld(status)
        if not report.get("identity"):
            return None  # The receiver rewrites its bounded live JSON snapshot.
        assert report["identity"] == identity, "The receive-only process identity changed during positioning"
        recent = status.get("recentApplication") or {}
        cursor = report.get("clientCursor") or {}
        acknowledged = report.get("focusRequests", 0) > previous_requests
        exact_foreground = (recent.get("pid") == identity["pid"] and recent.get("created") == identity["created"]
            and os.path.normcase(recent.get("executable", "")) == os.path.normcase(identity["executable"]))
        if acknowledged and cursor.get("inside") is True and exact_foreground:
            return dict(previousFocusRequests=previous_requests, focusRequests=report["focusRequests"],
                clientCursor=cursor, recentApplication=recent, focusMethod=report.get("focusMethod"),
                focusBefore=report.get("focusBefore"), focusAfter=report.get("focusAfter"))
        return None
    return mapping.wait_for(positioned, "Fresh receiver focus-and-center ACK, inside cursor and exact native foreground identity were not established", seconds=8)


def send(peer, value):
    peer.input([dict(controlId=CONTROL, value=value)])


def neutral(service, peers):
    for peer in peers:
        send(peer, False)
    mapping.wait_for(lambda: not native.desktop_status(service)["runtime"]["waitingNeutral"], "Fresh source neutral did not arm both routes")


def reopen_with_baseline_proof(peer, instance, publication):
    """Use the real WSS protocol to prove a new session cannot start held."""
    peer.paused.set()
    with peer.lock:
        assert peer.opened is None and peer.failure is None
        opened = peer.socket.request(companion.open_request(instance, publication))
        assert opened["ok"], opened
        peer.instance, peer.publication, peer.opened = instance, publication, opened
        peer.sequence = peer.heartbeat = 0
        peer.baseline_session = None
        unarmed = peer.socket.request(peer.message("input", sequence="1", changes=[dict(controlId=CONTROL, value=True)]))
        assert unarmed["ok"] is False and "neutral baseline" in unarmed["error"].lower(), unarmed
        controls = json.loads(publication["canonicalDocument"])["ui"]["controls"]
        values = {control["id"]: False if control["kind"] == "button" else control.get("fallback", -1) for control in controls}
        held = peer.socket.request(peer.message("baseline", sequence="0", values={**values, CONTROL: True}))
        assert held["ok"] is False and "declared fallback" in held["error"].lower(), held
        baseline = peer.socket.request(peer.message("baseline", sequence="0", values=values))
        assert baseline["ok"], baseline
        peer.baseline_session = opened["sessionId"]
        evidence = dict(session={key: opened[key] for key in ("clientId", "sessionId", "generation", "controllerInstanceId", "publicationHash")},
            inputBeforeBaseline=unarmed, heldBaseline=held, neutralBaseline=baseline)
    peer.paused.clear()
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bridge", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--receiver", type=Path, required=True)
    parser.add_argument("--desktop-settings-file", type=Path, required=True)
    parser.add_argument("--inject", action="store_true")
    args = parser.parse_args()
    assert args.inject and os.environ.get("OSG_DESKTOP_OWNERSHIP_NATIVE") == "1" and os.environ.get("OSG_DESKTOP_NATIVE_INJECT") == "1", "Actual native ownership acceptance requires both explicit opt-ins"
    service = native.NativeCockpit(args.bridge)
    before = service.profiles("profile_status")
    assert before["profileSession"]["state"] == "stopped" and before["profileSession"]["owner"] is None, "Stop the existing profile before this isolated acceptance"
    desktop_before = native.desktop_status(service)
    assert desktop_before["qualified"], "An explicitly qualified development build is required"
    # Restoring Automatic through its command would authorize automatic starts.
    # Require the current operator-selected Manual state for this isolated test.
    assert before["session"]["selectionMode"] == "manual", "This acceptance preserves a Manual selection without authorizing automatic activation"
    settings_before = args.desktop_settings_file.read_bytes() if args.desktop_settings_file.exists() else None
    original = json.loads((ROOT / "scripts/fixtures/compiled-flight-publication.json").read_text())
    changed = replacement(original)
    instances = ["desktop-owner-" + key + "-" + uuid.uuid4().hex[:10] for key in ("a", "b")]
    configurations = [(instance, original) for instance in instances]
    peers, saved, assigned = [], None, set()
    apply_attempted = False
    listener_enabled = companion.operator(service, "companion_status").get("listener", {}).get("enabled", False)
    evidence = dict(actualNativeCockpit=True, actualWssPeers=2, injected=False, virtualControllers=0, productionQualified=False,
        measurement="Receiver-observed marked MouseLeft messages and native per-action ownership/ACKs; no game behavior, physical-controller or latency claim.",
        checks=[], stages={}, profileId=None)
    pattern = []
    pins = None
    def wait(owners, message):
        assert pins is not None
        return wait_owned(service, args.receiver, owners, pattern, message, pins)
    try:
        if not listener_enabled:
            with socket.socket() as probe:
                probe.bind(("127.0.0.1", 0))
                port = probe.getsockname()[1]
            companion.operator(service, "companion_enable", address="127.0.0.1", advertisedHost="127.0.0.1", port=port, discovery=False)
        companion.publish(service, original)
        companion.publish(service, changed)
        for instance in instances:
            peer = TransportPeer(service, instance)
            peers.append(peer)
            companion.operator(service, "companion_assign", companionId=instance, instanceId=instance, publicationHash=original["publicationHash"])
            assigned.add(instance)
            peer.open(instance, original)
        document, assignments, details = profile_for(service, configurations)
        saved = mapping.save(service, document, assignments)
        evidence["profileId"] = saved["id"]
        compiled = mapping.state(service, "profile_compile", "profileCompilation", document=saved, assignments=assignments)
        assert compiled["readiness"] != "invalid", compiled
        assert compiled["requirements"] == dict(desktopInput=True, virtualDevices=False, visibility=False), compiled
        assert compiled["budgets"]["virtualTargets"] == 0
        evidence.update(profile=saved, assignments=assignments, compilation=compiled)
        identity = mapping.wait_for(lambda: native.receiver(args.receiver).get("identity"), "Receive-only window is not ready")
        assert mouse_edges(args.receiver) == [], "Receiver contains prior marked mouse messages"
        evidence["receiverPositioning"] = dict(initial=position_receiver(service, args.receiver, identity))
        captured = mapping.state(service, "profile_desktop_capture_application", "desktop")
        reference = captured['capturedApplicationReference']
        assert captured['recentApplication'] == identity and reference
        # WebView JSON numbers cannot carry all FILETIME bits. The native file
        # preserves the exact u64 creation epoch which the host will consume.
        captured_file = json.loads(args.desktop_settings_file.read_text())["applications"][reference]
        assert captured_file["pid"] == identity["pid"] and int(captured_file["epoch"]) == int(identity["created"]), captured_file
        mapping.state(service, "profile_desktop_configure", "desktop", enabled=True, scope=dict(kind="application"))
        document['desktopOutputs'][0]['scope'] = dict(kind='application', applicationReference=reference)
        saved = mapping.save(service, document, assignments, expected=saved['revision'])
        service.profiles("profile_stop")
        apply_attempted = True
        owner = mapping.apply(service, saved)
        evidence["receiverPositioning"]["initialApplied"] = position_receiver(service, args.receiver, identity)
        neutral(service, peers)
        evidence["initialSources"] = checkpoint(service, peers, configurations)
        pins = producer_pins(owner, evidence["initialSources"], instances)
        evidence["checks"].append("Two independently authenticated WSS peers bind exact native source identities to two MouseLeft actions in one applied desktop-only profile")

        send(peers[0], True)
        pattern.append(513)
        wait({"route-a"}, "Source A did not establish the first native MouseLeft owner")
        send(peers[1], True)
        both = wait({"route-a", "route-b"}, "Source B did not share MouseLeft without another down")
        pins["stable"]["route-b"] = held_producers(both, pins)["route-b"]
        collisions = [item for item in both["runtime"]["outcomes"] if item["delivery"]["actionId"] == "route-b" and item["delivery"]["intent"] == dict(kind="held", controls=[LEFT])]
        assert collisions and collisions[-1]["acknowledgement"]["collision"] is True, collisions
        assert collisions[-1]["acknowledgement"]["attempted"] == collisions[-1]["acknowledgement"]["confirmed"] == 0
        send(peers[0], False)
        first_release = wait({"route-b"}, "A release revoked B's MouseLeft ownership")
        send(peers[0], True)
        wait({"route-a", "route-b"}, "A did not rejoin B's held mouse")
        refused = service.request("companion_assign", companion=dict(companionId=instances[0], instanceId=instances[0], publicationHash=changed["publicationHash"]))
        assert refused["ok"] is False and "unified profile session" in refused["error"].lower(), refused
        refusal_held = wait({"route-a", "route-b"}, "Refused independent publication write disturbed shared ownership")
        peers[0].end()
        lost = wait({"route-b"}, "Only A's WSS session loss must revoke A while preserving B")
        send(peers[1], False)
        pattern.append(514)
        final = wait(set(), "Final B release did not produce exactly one MouseUp")
        evidence["stages"]["independentLoss"] = dict(both=both, firstRelease=first_release, blockedPublicationWrite=refused, afterBlockedWrite=refusal_held, afterSourceALoss=lost, finalRelease=final)
        evidence["checks"].append("Shared MouseLeft has one down; A release, refused independent publication write and A WSS close preserve B; final B release has exactly one up")

        # B remains live while only A opens a new session. Exercise the actual
        # protocol's prebaseline refusals, not Peer.open's automatic neutral.
        send(peers[1], True)
        pattern.append(513)
        wait({"route-b"}, "B did not remain usable after A loss")
        baseline_proof = reopen_with_baseline_proof(peers[0], instances[0], original)
        assert baseline_proof["session"]["sessionId"] != evidence["initialSources"][instances[0]]["session"]["sessionId"]
        companion.wait_native_companions(service, configurations[:1], peers[:1],
            previous={instances[0]: evidence["initialSources"][instances[0]]})
        companion.wait_native_companions(service, configurations, peers)
        refreshed = checkpoint(service, peers, configurations)
        assert refreshed[instances[0]]["generation"] > evidence["initialSources"][instances[0]]["generation"], refreshed
        assert refreshed[instances[1]] == evidence["initialSources"][instances[1]], "A session replacement changed B's exact native source/session generation"
        pins["generations"]["route-a"] = str(refreshed[instances[0]]["generation"])
        send(peers[0], False)
        wait({"route-b"}, "A fresh neutral disturbed B's native hold")
        send(peers[0], True)
        rejoined = wait({"route-a", "route-b"}, "Fresh neutral then A press did not rejoin B under the new source generation")
        send(peers[0], False)
        wait({"route-b"}, "New-generation A release revoked B")
        send(peers[1], False)
        pattern.append(514)
        wait(set(), "Final B release after A's new generation did not produce exactly one up")
        evidence["stages"]["sessionReplacement"] = dict(protocol=baseline_proof, refreshedSources=refreshed, rejoined=rejoined,
            heldBProducerBefore=pins["stable"]["route-b"], heldBProducerAfter=held_producers(rejoined, pins)["route-b"])
        evidence["checks"].append("A new WSS/native provider generation rejects true before baseline and a held baseline, accepts a complete neutral baseline, then can share B without replacing B's session or native generation")

        # A real transport failure is distinct from the acknowledged protocol
        # close above. B remains held throughout A's new authenticated connection
        # and baseline admission, so any accidental release is receiver-visible.
        transport_sources = checkpoint(service, peers, configurations)
        send(peers[0], True)
        pattern.append(513)
        wait({"route-a"}, "A did not establish the transport-loss MouseLeft hold")
        send(peers[1], True)
        transport_both = wait({"route-a", "route-b"}, "A and B did not share MouseLeft before abrupt transport loss")
        transport_b_producer = held_producers(transport_both, pins)["route-b"]
        transport_stage = dict(beforeSources=transport_sources, both=transport_both, heldBProducerBefore=transport_b_producer)
        evidence["stages"]["abruptTransportLoss"] = transport_stage
        aborted = peers[0].abort_transport()
        transport_stage["shutdown"] = aborted
        transport_lost = wait({"route-b"}, "Abrupt A TCP shutdown did not revoke only A's native contribution")
        transport_stage["afterLoss"] = transport_lost
        assert held_producers(transport_lost, pins)["route-b"] == transport_b_producer
        authenticated = peers[0].reconnect_transport(aborted)
        transport_stage["reconnect"] = authenticated
        transport_baseline = reopen_with_baseline_proof(peers[0], instances[0], original)
        transport_stage["protocol"] = transport_baseline
        assert transport_baseline["session"]["sessionId"] != aborted["oldSession"]["sessionId"]
        # Source-accepted WSS ACKs precede the asynchronous native resource
        # inventory publication. Wait for A's actual newer route generation;
        # waiting for both peers to replace would wrongly force B to reconnect.
        transport_stage["firstNativeCheckpointAfterBaseline"] = checkpoint(service, peers, configurations)
        companion.wait_native_companions(service, configurations[:1], peers[:1],
            previous={instances[0]: transport_sources[instances[0]]})
        companion.wait_native_companions(service, configurations, peers)
        transport_refreshed = checkpoint(service, peers, configurations)
        transport_stage["afterSources"] = transport_refreshed
        assert transport_refreshed[instances[0]]["generation"] > transport_sources[instances[0]]["generation"]
        assert transport_refreshed[instances[1]] == transport_sources[instances[1]], "A's new transport replaced B's exact native source/session generation"
        pins["generations"]["route-a"] = str(transport_refreshed[instances[0]]["generation"])
        after_transport_neutral = wait({"route-b"}, "A transport authentication or neutral baseline disturbed B's native hold")
        send(peers[0], True)
        transport_rejoined = wait({"route-a", "route-b"}, "Neutral-gated A could not rejoin B over its new authenticated transport")
        send(peers[0], False)
        after_transport_release = wait({"route-b"}, "Reconnected A release revoked B's original producer")
        send(peers[1], False)
        pattern.append(514)
        transport_final = wait(set(), "Final B release after abrupt A transport loss did not produce exactly one MouseUp")
        transport_stage.update(afterNeutral=after_transport_neutral, rejoined=transport_rejoined,
            afterARelease=after_transport_release, finalBRelease=transport_final,
            heldBProducerAfter=held_producers(after_transport_release, pins)["route-b"])
        evidence["checks"].append("A's underlying TCP shutdown sends no protocol/WebSocket close; only A revokes while B's exact held producer persists without an extra native edge; a new transport reuses A's credential, requires full neutral, rejoins B, and final B release produces one up")

        # Publication assignment is an explicit stopped-state operation. Keep
        # that admission boundary visible; it is not live publication mutation.
        before_stop = checkpoint(service, peers, configurations)
        service.profiles("profile_stop")
        companion.wait_native_companions(service, configurations, peers, previous=before_stop)
        stopped_sources = checkpoint(service, peers, configurations)
        peers[0].paused.set()
        try:
            # paused alone cannot exclude a heartbeat already past its check.
            # The same lock protects assignment plus the real open/baseline.
            with peers[0].lock:
                assert peers[0].failure is None, peers[0].failure
                companion.operator(service, "companion_assign", companionId=instances[0], instanceId=instances[0], publicationHash=changed["publicationHash"])
                peers[0].instance, peers[0].publication = instances[0], changed
                peers[0]._open_locked(instances[0], changed)
        finally:
            peers[0].paused.clear()
        configurations[0] = (instances[0], changed)
        companion.wait_native_companions(service, configurations, peers)
        replacement_sources = checkpoint(service, peers, configurations)
        assert replacement_sources[instances[1]] == stopped_sources[instances[1]], "Replacing A's publication changed B's session or native generation"
        assert replacement_sources[instances[0]]["generation"] > stopped_sources[instances[0]]["generation"]
        document, assignments, _ = profile_for(service, configurations, previous=saved)
        saved = mapping.save(service, document, assignments, expected=saved["revision"])
        evidence["receiverPositioning"]["beforePublicationApply"] = position_receiver(service, args.receiver, identity)
        owner_after = mapping.apply(service, saved)
        assert owner_after != owner
        evidence["receiverPositioning"]["publicationApplied"] = position_receiver(service, args.receiver, identity)
        neutral(service, peers)
        applied_sources = checkpoint(service, peers, configurations)
        pins = producer_pins(owner_after, applied_sources, instances)
        send(peers[1], True)
        pattern.append(513)
        replacement_b = wait({"route-b"}, "B did not work after explicit publication replacement Apply")
        pins["stable"]["route-b"] = held_producers(replacement_b, pins)["route-b"]
        send(peers[0], True)
        replaced_both = wait({"route-a", "route-b"}, "Replacement publication A did not share B's native hold")
        peers[0].end()
        replaced_loss = wait({"route-b"}, "Replacement-publication source A loss revoked B")
        send(peers[1], False)
        pattern.append(514)
        wait(set(), "Final B release after publication replacement did not produce one native up")
        evidence["stages"]["stoppedPublicationReplacement"] = dict(beforeSources=stopped_sources, afterSources=replacement_sources,
            appliedSources=applied_sources, revision=saved["revision"], appliedOwner=owner_after, both=replaced_both, afterSourceALoss=replaced_loss,
            heldBProducerBefore=pins["stable"]["route-b"], heldBProducerAfter=held_producers(replaced_loss, pins)["route-b"])
        evidence["checks"].append("After clean Stop only A receives a different immutable publication; B's session/generation stays exact until explicit Apply, then independent shared ownership and final release work again")
        evidence.update(injected=True, appliedOwner=owner, mouseMessages=pattern, receiver=native.receiver(args.receiver))
    except Exception as error:
        evidence["failure"] = str(error)
        evidence["receiver"] = native.receiver(args.receiver)
        try:
            evidence["runtimeAtFailure"] = native.desktop_status(service)
            evidence["profileAtFailure"] = service.profiles("profile_status")
        except Exception as observation:
            evidence["failureObservationError"] = str(observation)
        raise
    finally:
        errors = []
        def cleanup(label, operation):
            try:
                return operation()
            except Exception as error:
                errors.append(label + ": " + str(error))
        cleanup("Stop native profile", lambda: service.profiles("profile_stop"))
        for peer in peers:
            cleanup("Close test WSS peer", peer.close)
        def prove_stopped():
            status, profile = native.desktop_status(service), service.profiles("profile_status")
            assert profile["profileSession"]["state"] == "stopped" and profile["profileSession"]["owner"] is None, profile
            if status.get("runtime") is None:
                assert not apply_attempted, "No native ledger proves cleanup after attempted Apply"
            else:
                state = backend(status)
                assert state["held"] == state["possibleHeld"] == state["owners"] == [] and state["cleanupComplete"], state
            evidence["ownershipCleanupProof"] = dict(profileSession=profile["profileSession"], desktop=status)
            return True
        ownership_clear = cleanup("Prove native ownership cleanup", prove_stopped) is True
        evidence["ownershipCleanupConfirmed"] = ownership_clear
        evidence["needsRootCleanup"] = not ownership_clear
        if ownership_clear:
            cleanup("Restore desktop preferences", lambda: mapping.state(service, "profile_desktop_configure", "desktop", enabled=desktop_before["enabled"], scope=desktop_before["scope"]))
            def restore_settings():
                if settings_before is None:
                    args.desktop_settings_file.unlink(missing_ok=True)
                else:
                    args.desktop_settings_file.write_bytes(settings_before)
            cleanup("Restore exact desktop preference bytes", restore_settings)
            cleanup("Restore selected profile without activation", lambda: service.profiles("profile_select", id=before["session"]["selectedProfileId"], selectionSource="manual"))
            if saved:
                deleted = cleanup("Delete only exact test profile revision", lambda: service.profiles("profile_delete", id=saved["id"], expectedRevision=saved["revision"]))
                evidence["deletedTestProfile"] = deleted
            for instance in assigned:
                cleanup("Unassign test instance", lambda instance=instance: companion.operator(service, "companion_unassign", instanceId=instance))
            for instance in instances[:len(peers)]:
                cleanup("Revoke test WSS identity", lambda instance=instance: companion.operator(service, "companion_revoke", companionId=instance))
            if not listener_enabled:
                cleanup("Restore disabled listener", lambda: companion.operator(service, "companion_disable"))
        def verify():
            status, profile = native.desktop_status(service), service.profiles("profile_status")
            state = backend(status)
            assert state["held"] == state["possibleHeld"] == state["owners"] == [] and state["cleanupComplete"]
            assert profile["profileSession"]["state"] == "stopped" and profile["profileSession"]["owner"] is None
            assert profile["session"]["selectedProfileId"] == before["session"]["selectedProfileId"]
            assert profile["session"]["selectionMode"] == before["session"]["selectionMode"]
            assert status["enabled"] == desktop_before["enabled"] and status["scope"] == desktop_before["scope"] and status["application"] == desktop_before["application"]
            listener = companion.operator(service, "companion_status").get("listener", {})
            assert listener.get("enabled", False) == listener_enabled
            library = service.profiles("profile_list")["profiles"]
            assert saved is None or all(item["id"] != saved["id"] for item in library)
            evidence["finalStatus"] = dict(profileSession=profile["profileSession"], session=profile["session"], desktop=status, listener=listener)
        if ownership_clear:
            cleanup("Verify stopped cleanup and restored selection/preferences/listener", verify)
        evidence["cleanupErrors"] = errors
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
        if errors and sys.exc_info()[0] is None:
            raise RuntimeError("; ".join(errors))
    print(json.dumps(dict(profileId=evidence["profileId"], injected=evidence["injected"], checks=evidence["checks"], mouseMessages=pattern), indent=2))


if __name__ == "__main__":
    main()
