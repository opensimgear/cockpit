#!/usr/bin/env python3
"""Opt-in real WSS workload for native QPC audit; this file computes no latency.

The running ProfileSession is the only injector. Python send times describe the
workload, never capture-to-submit time. Native audit provenance supplies that
separate boundary; receiver observations remain an independent delivery check.
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import socket
import sys
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("latency_ownership_helpers", ROOT / "scripts/desktop-ownership-native.py")
ownership = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ownership)
native, companion, mapping = ownership.native, ownership.companion, ownership.mapping
BUTTON, AXIS = "flight-input-0", "flight-input-2"
RATE, SECONDS = 80, 6
KEYS = [(4, 30), (5, 48)]  # HID usage and Windows physical scan code.
AXIS_FACTOR, AXIS_OFFSET, AXIS_NEUTRAL = 1 / 16384, -1, 0.5


def compiled_publication():
    publication = json.loads((ROOT / "scripts/fixtures/compiled-flight-publication.json").read_text())
    document = json.loads(publication["canonicalDocument"])
    assert document["schemaVersion"] == 4 and document["presentation"]["format"] == "compiled-raster-v1"
    controls = {control["id"]: control for control in document["ui"]["controls"]}
    assert controls[BUTTON]["kind"] == "button"
    assert controls[AXIS]["kind"] == "axis" and controls[AXIS]["axisMode"] == "unipolar"
    bindings = [binding for widget in document["presentation"]["widgets"] for binding in widget["inputBindings"]]
    assert dict(controlId=BUTTON, portId="button") in bindings
    assert dict(controlId=AXIS, portId="axis") in bindings
    return publication


def axis_wire(signed):
    assert -1 <= signed <= 1
    return (signed + 1) / 2


def receiver_injection_observed(paths):
    return any(event.get("injectedMarker") is True for path in paths
        for event in native.receiver(path).get("observations", []))


def capture_profile_scope(service, saved, assignments, identity):
    captured = mapping.state(service, "profile_desktop_capture_application", "desktop")
    assert captured["recentApplication"] == identity, captured
    reference = captured["capturedApplicationReference"]
    assert isinstance(reference, str) and reference, captured
    document = json.loads(json.dumps(saved))
    for output in document["desktopOutputs"]:
        output["scope"] = dict(kind="application", applicationReference=reference)
    updated = mapping.save(service, document, assignments, expected=saved["revision"])
    assert updated["id"] == saved["id"] and updated["revision"] != saved["revision"], updated
    compilation = mapping.state(service, "profile_compile", "profileCompilation", document=updated, assignments=assignments)
    assert compilation["readiness"] != "invalid" and compilation["requirements"] == dict(desktopInput=True, virtualDevices=False, visibility=False), compilation
    assert compilation["budgets"]["virtualTargets"] == 0
    return updated, captured, compilation


def decimal(value):
    return isinstance(value, str) and value.isdecimal() and str(int(value)) == value


def audit_checkpoint(service):
    result = mapping.state(service, "profile_desktop_audit_checkpoint", "desktop")["audit"]
    assert all(result[key] is True for key in ("configured", "enabled", "complete", "valid")), result
    assert decimal(result["throughSequence"]), result
    counters = result["counters"]
    assert all(decimal(value) for value in counters.values()), counters
    assert int(counters["written"]) < 65_536, "The audit record bound was reached"
    return result


def marker(service, phase):
    value = mapping.state(service, "profile_desktop_audit_marker", "desktop", phase=phase)["auditMarker"]
    assert decimal(value["id"]) and int(value["id"]) > 0 and value["phase"] == phase, value
    return value


def control(service, command, **fields):
    reply = service.request(command, **fields)
    assert reply["ok"], reply
    return reply["state"]["latency"]


def send(peer, changes):
    with peer.lock:
        assert peer.failure is None, peer.failure
        peer.sequence += 1
        result = peer.socket.request(peer.message("input", sequence=str(peer.sequence), changes=changes))
        assert result["ok"] and result["ackStage"] == "source-accepted" and result["submitted"] is False, result
        assert result["sequence"] == str(peer.sequence), result
        return str(peer.sequence)


def axis_boundary(contract):
    schema = contract["schema"]
    assert schema["type"] == "integer" and schema["min"] == 0 and schema["max"] == 32767, contract
    return dict(physicalControl=AXIS, signedWorkloadRange=[-1, 1], signedWorkloadAmplitude=0.65,
        wireRange=[0, 1], wireEncoding="(signed + 1) / 2", wireNeutral=AXIS_NEUTRAL,
        nativeRange=[schema["min"], schema["max"]], explicitScaleFactor=AXIS_FACTOR, explicitScaleOffset=AXIS_OFFSET,
        conversion="The supported compiled unipolar WSS axis quantizes round(wire * 32767). The graph computes raw / 16384 - 1, centering neutral 0.5 exactly; the signed waveform has bounded quantization error.")


def current_companion_resource(service, instance):
    detail = companion.selected_resource(service, instance, require_current=False)
    return detail if detail and all(port["current"] for port in detail["ports"]) else None


def profile_for(service, instances, publication, keys_only=False):
    document = mapping.blank("Native desktop latency " + uuid.uuid4().hex[:8])
    assignments = []
    boolean = dict(delivery="state", schema=dict(type="boolean"), freshnessMs=1000, fallback=False)
    number = dict(delivery="state", schema=dict(type="number", min=-1, max=1, unit=None), freshnessMs=1000, fallback=0)
    actions, outputs = [], []
    for index, instance in enumerate(instances):
        logical = "a" if index == 0 else "b"
        source = "panel-" + logical
        detail = mapping.wait_for(lambda: current_companion_resource(service, instance), "No current native companion ports")
        inputs = [("button", BUTTON)] if keys_only else [("button", BUTTON), ("axis", AXIS)]
        observed = {name: next(port for port in detail["ports"] if port["descriptor"]["id"] == physical)
            for name, physical in inputs}
        document["resources"].append(dict(id=source, name="Real WSS " + logical.upper(), kind="companion", required=False,
            ports=[mapping.port(name, port["descriptor"]["contract"], "output") for name, port in observed.items()]))
        document["companionAssignments"].append(dict(resourceId=source, publicationHash=publication["publicationHash"]))
        assignments.append(dict(resourceId=source, sourceId=detail["resource"]["id"], ports=[dict(profilePortId=name,
            observedPortId=port["descriptor"]["id"], expectedContractHash=port["contractHash"]) for name, port in observed.items()]))
        key = "key-" + logical
        outputs.append(mapping.port(key, boolean, "input"))
        actions.append(dict(portId=key, action=dict(kind="key-hold", key=KEYS[index][0])))
        document["bindings"].append(dict(id=key, **{"from":dict(nodeId=source, portId="button"), "to":dict(nodeId="desktop", portId=key)}))
        if keys_only:
            continue
        axis_boundary(observed["axis"]["descriptor"]["contract"])
        normalizer = "normalize-" + logical
        document["processingNodes"].append(dict(id=normalizer, name="Explicit raw axis normalization " + logical,
            operation=dict(kind="scale", factor=AXIS_FACTOR, offset=AXIS_OFFSET),
            inputs=[mapping.port("in", observed["axis"]["descriptor"]["contract"], "input")], outputs=[mapping.port("out", number, "output")]))
        outputs.append(mapping.port("pointer-" + logical, number, "input"))
        actions.append(dict(portId="pointer-" + logical, action=dict(kind="pointer", axis="x" if index == 0 else "y", speed=40, curve=1)))
        for identity, start_node, start_port, end_node, end_port in [
            ("axis-" + logical, source, "axis", normalizer, "in"),
            ("pointer-" + logical, normalizer, "out", "desktop", "pointer-" + logical)]:
            document["bindings"].append(dict(id=identity, **{"from":dict(nodeId=start_node, portId=start_port), "to":dict(nodeId=end_node, portId=end_port)}))
    document["desktopOutputs"] = [dict(id="desktop", name="Immediate KeyA/KeyB" if keys_only else "Immediate KeyA/KeyB and pointer load", required=True, ports=outputs, actions=actions)]
    return document, assignments


def workload(peer, index, gate, abort, results, errors, seconds=SECONDS, keys_only=False):
    record = dict(companionId=peer.name, logicalSource="panel-" + ("a" if index == 0 else "b"),
        buttonPort="button", physicalButton=BUTTON, axisPort="axis", physicalAxis=AXIS,
        keyHid=KEYS[index][0], scanCode=KEYS[index][1], actionId="key-" + ("a" if index == 0 else "b"),
        configuredDelayUs="0", requestedRateHz=RATE, frames=[], keyChanges=[], schedulerLateIntervals=0)
    results[index] = record
    try:
        gate.wait(timeout=5)
        started = time.perf_counter()
        due, tick, pressed = started, 0, False
        while not abort.is_set() and time.perf_counter() - started < seconds:
            abort.wait(max(0, due - time.perf_counter()))
            if abort.is_set() or time.perf_counter() - started >= seconds:
                break
            # Encode the original signed waveform into the supported compiled
            # unipolar wire domain; the graph restores signed pointer load.
            axis = round(0.65 * math.sin((tick + 1) * math.pi / 40), 6)
            changes = [] if keys_only else [dict(controlId=AXIS, value=axis_wire(axis))]
            desired = (tick // 4) % 2 == 0
            if desired != pressed:
                changes.append(dict(controlId=BUTTON, value=desired))
            if changes:
                sequence = send(peer, changes)
                record["frames"].append(dict(wireSequence=sequence, changes=changes))
            if desired != pressed:
                record["keyChanges"].append(dict(wireSequence=sequence, down=desired))
                pressed = desired
            tick += 1
            due += 1 / RATE
            if time.perf_counter() > due:
                record["schedulerLateIntervals"] += 1
                due = time.perf_counter() + 1 / RATE  # No catch-up burst.
        record.update(inputFrames=len(record["frames"]), axisChanges=0 if keys_only else len(record["frames"]),
            expectedImmediateKeyChanges=len(record["keyChanges"]), finalPressed=pressed,
            workloadElapsedSeconds=time.perf_counter() - started)
    except BaseException as error:
        errors.append(str(error))
        abort.set()


def key_edges(report, scan):
    return [event for event in report.get("observations", []) if event.get("injectedMarker") is True
        and event["message"] in (256, 257) and event["scanCode"] == scan and event["extended"] == 0]


def pointer_load_proof(report, status, observation_start, operation_baseline):
    operation_baseline = int(operation_baseline)
    moves = [event for event in report.get("observations", [])[observation_start:]
        if event.get("injectedMarker") is True and event["message"] == 512
        and event["foregroundWindow"] == report["focusAfter"] and event["captureWindow"] == "0"]
    receipts = {action: [] for action in ("pointer-a", "pointer-b")}
    for outcome in status["runtime"].get("outcomes", []):
        delivery, ack = outcome["delivery"], outcome["acknowledgement"]
        assert decimal(delivery["operationId"]), delivery
        if (int(delivery["operationId"]) <= operation_baseline or delivery["actionId"] not in receipts
                or delivery["intent"]["kind"] != "pointer" or ack["attempted"] == 0):
            continue
        assert ack["ok"] and not ack["uncertain"] and not ack["skipped"] and ack["confirmed"] == ack["attempted"], outcome
        receipts[delivery["actionId"]].append(outcome)
    if not moves or not all(receipts.values()):
        return None
    return dict(receiverMarkedMouseMoves=len(moves), receiverObservationStart=observation_start,
        operationBaseline=str(operation_baseline),
        countBoundary="Native counts cover the bounded recent outcome buffer after warmup, not the whole phase; complete native submissions remain in the audit trace.",
        recentNativePointerCounts={action: dict(successfulSubmissions=len(values),
            nativeEventsAttempted=sum(value["acknowledgement"]["attempted"] for value in values),
            nativeEventsConfirmed=sum(value["acknowledgement"]["confirmed"] for value in values)) for action, values in receipts.items()},
        recentNativePointerReceipts=receipts)


def settle_keys(service, receiver_path, results, receiver_prefix=None, pointer_baseline=None):
    receiver_prefix = receiver_prefix or {}
    def ready():
        report = native.receiver(receiver_path)
        if "observations" not in report:  # The live snapshot may be between truncate and write.
            return None
        assert len(report.get("observations", [])) < 4096, "Receiver observation capacity reached"
        status = native.desktop_status(service)
        state = status["runtime"]["backend"]
        assert not state["uncertain"] and state["error"] is None, state
        assert status["runtime"]["error"] is None and status["runtime"]["scopeError"] is None, status
        assert status["runtime"]["state"] == "active" and status["runtime"]["waitingNeutral"] == [], json.dumps({
            "message":"The workload lost its armed application scope", "state":status["runtime"]["state"],
            "waitingNeutral":status["runtime"]["waitingNeutral"], "scopeNotifications":status["runtime"].get("scopeNotifications"),
            "helperScope":status["runtime"].get("helperScope"), "recentApplication":status.get("recentApplication"), "backendPhase":state.get("phase"),
            "runs":state.get("runs"), "held":state.get("held")})
        if state["inFlight"] is not None:
            return None
        assert state["possibleHeld"] == state["held"]
        key_counts = []
        for record in results:
            events = key_edges(report, record["scanCode"])
            prefix = receiver_prefix.get(str(record["scanCode"]), [])
            assert events[:len(prefix)] == prefix, "The exact receiver warmup prefix changed or disappeared"
            events = events[len(prefix):]
            expected = [256 if change["down"] else 257 for change in record["keyChanges"]]
            actual = [event["message"] for event in events]
            assert actual == expected[:len(actual)], "Unexpected, reordered or repeated native key edge"
            if actual != expected:
                return None
            owners = [owner for owner in state["owners"] if owner["actionId"] == record["actionId"]]
            held = [dict(kind="key", key=record["keyHid"])] if record["finalPressed"] else []
            if len(owners) != 1 or owners[0]["controls"] != held:
                return None
            key_counts.append(dict(actionId=record["actionId"], warmupEdgesExcluded=len(prefix),
                expectedChangedEdges=len(expected), observedChangedEdges=len(actual)))
        pointer = None
        if pointer_baseline is not None:
            pointer = pointer_load_proof(report, status, **pointer_baseline)
            if pointer is None:
                return None
        return dict(receiver=report, desktop=native.compact_desktop(status), keyCounts=key_counts, pointerLoad=pointer)
    message = "Accepted keys, exact ownership and required pointer receipts did not settle" if pointer_baseline is not None else "Accepted keys and exact ownership did not settle"
    return mapping.wait_for(ready, message, seconds=8)


def warmup_keys(service, receiver_path, peers):
    """Establish actual producers before the measured marker, with exact edges."""
    records = [dict(companionId=peer.name, scanCode=KEYS[index][1], keyHid=KEYS[index][0],
        actionId="key-" + ("a" if index == 0 else "b"), keyChanges=[], finalPressed=False)
        for index, peer in enumerate(peers)]
    stages = []
    for down in (True, False):
        for peer, record in zip(peers, records):
            sequence = send(peer, [dict(controlId=BUTTON, value=down)])
            record["keyChanges"].append(dict(wireSequence=sequence, down=down))
            record["finalPressed"] = down
        settled = settle_keys(service, receiver_path, records)
        stages.append(dict(down=down, settled=settled))
    report = settled["receiver"]
    receiver_prefix = {str(record["scanCode"]): key_edges(report, record["scanCode"]) for record in records}
    status = native.desktop_status(service)
    operations = [int(outcome["delivery"]["operationId"]) for outcome in status["runtime"].get("outcomes", [])]
    return dict(sources=records, stages=stages, settled=settled, receiverKeyPrefix=receiver_prefix,
        pointerBaseline=dict(observation_start=len(report["observations"]), operation_baseline=str(max(operations, default=0))))


def neutral_owners(service, keys_only=False):
    runtime = native.desktop_status(service)["runtime"]
    backend = runtime["backend"]
    if runtime["state"] != "active" or runtime["waitingNeutral"] or backend["inFlight"] is not None:
        return None
    owners = [owner for owner in backend["owners"] if not owner["controls"]]
    expected = {"key-a", "key-b"} if keys_only else {"key-a", "key-b", "pointer-a", "pointer-b"}
    return owners if {owner["actionId"] for owner in owners} >= expected else None


def coherent_sources(service, configurations, peers):
    """Correlate asynchronous inventory with the actual applied typed producers."""
    def ready():
        snapshot = companion.companion_checkpoint(service, configurations, peers)
        owners = native.desktop_status(service)["runtime"]["backend"]["owners"]
        for logical, (instance, _publication) in zip(("a", "b"), configurations):
            keys = [owner for owner in owners if owner["actionId"] == "key-" + logical]
            if len(keys) != 1:
                return None
            dependencies = keys[0]["producer"]["dependencies"]
            if len(dependencies) != 1 or dependencies[0]["generation"] != str(snapshot[instance]["generation"]):
                return None
        return snapshot
    return mapping.wait_for(ready, "Native catalog generations did not match the actual applied source producers", seconds=8)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for field in ("bridge", "output", "app-sha256", "audit-directory", "audit-run"):
        parser.add_argument("--" + field, required=True, type=Path if field in ("output", "audit-directory") else str)
    parser.add_argument("--inject", action="store_true")
    parser.add_argument("--keys-only", action="store_true")
    args = parser.parse_args()
    assert args.inject and os.environ.get("OSG_DESKTOP_LATENCY_NATIVE") == os.environ.get("OSG_DESKTOP_NATIVE_INJECT") == "1", "Two explicit native latency opt-ins are required"
    assert len(args.app_sha256) == 64 and all(char in "0123456789abcdef" for char in args.app_sha256)
    service = native.NativeCockpit(args.bridge)
    before = service.profiles("profile_status")
    assert before["profileSession"]["state"] == "stopped" and before["profileSession"]["owner"] is None
    assert before["session"]["selectionMode"] == "manual"
    preferences = native.desktop_status(service)
    assert preferences["qualified"] and native.native_input_workers() == []
    listener_before = companion.operator(service, "companion_status").get("listener", {}).get("enabled", False)
    publication = compiled_publication()
    instances = ["desktop-latency-" + label + "-" + uuid.uuid4().hex[:10] for label in ("a", "b")]
    configurations = [(instance, publication) for instance in instances]
    peers, assigned, saved = [], set(), None
    receiver_paths = []
    apply_attempted = False
    evidence = dict(schemaVersion=1, actualNativeCockpit=True, actualWssPeers=2, injected=False, productionQualified=False, keysOnly=args.keys_only,
        nativeExecutableSha256=args.app_sha256, auditDirectory=str(args.audit_directory), auditRun=args.audit_run,
        measurement="Native audit host ingress to successful SendInput uses QPC; Python times describe workload only. Receiver/game timing is separate; this harness computes no percentiles.",
        phases=[], latencyPercentiles=None, checks=[])
    try:
        evidence["initialAuditCheckpoint"] = audit_checkpoint(service)
        if not listener_before:
            with socket.socket() as probe:
                probe.bind(("127.0.0.1", 0))
                port = probe.getsockname()[1]
            companion.operator(service, "companion_enable", address="127.0.0.1", advertisedHost="127.0.0.1", port=port, discovery=False)
        companion.publish(service, publication)
        for instance in instances:
            peer = companion.Peer(service, instance)
            peers.append(peer)
            companion.operator(service, "companion_assign", companionId=instance, instanceId=instance, publicationHash=publication["publicationHash"])
            assigned.add(instance)
            peer.open(instance, publication)
        document, assignments = profile_for(service, instances, publication, args.keys_only)
        neutral = [dict(controlId=BUTTON, value=False)]
        if not args.keys_only:
            neutral.append(dict(controlId=AXIS, value=AXIS_NEUTRAL))
        saved = mapping.save(service, document, assignments)
        compilation = mapping.state(service, "profile_compile", "profileCompilation", document=saved, assignments=assignments)
        assert compilation["readiness"] != "invalid" and compilation["requirements"] == dict(desktopInput=True, virtualDevices=False, visibility=False), compilation
        assert compilation["budgets"]["virtualTargets"] == 0
        evidence.update(profile=saved, assignments=assignments, compilation=compilation,
            publication=dict(publicationHash=publication["publicationHash"], fixture="scripts/fixtures/compiled-flight-publication.json",
                schemaVersion=4, format="compiled-raster-v1"),
            axisBoundaries={resource["id"]: axis_boundary(next(port["contract"] for port in resource["ports"] if port["id"] == "axis"))
                for resource in saved["resources"]} if not args.keys_only else {})
        for index, phase_name in enumerate(("idle", "ui-load")):
            phase = dict(name=phase_name, durationTargetSeconds=SECONDS, requestedAxisRateHzPerPeer=0 if args.keys_only else RATE)
            evidence["phases"].append(phase)
            target = control(service, "latency_receiver_begin", phase=phase_name)
            receiver_path, identity = Path(target["file"]), target["identity"]
            receiver_paths.append(receiver_path)
            phase["receiverIdentity"] = identity
            phase["initialForeground"] = ownership.position_receiver(service, receiver_path, identity,
                pointer_target=not args.keys_only, keyboard_window=target["window"] if args.keys_only else None)
            saved, captured, compilation = capture_profile_scope(service, saved, assignments, identity)
            phase["exactCapturedApplication"] = captured
            phase.update(profile=saved, compilation=compilation)
            evidence.update(profile=saved, compilation=compilation)
            mapping.state(service, "profile_desktop_configure", "desktop", enabled=True, scope=dict(kind="application"))
            service.profiles("profile_stop")
            # The publication's source fallback is raw zero. Establish the
            # signed neutral before Apply, then repeat it after its focus gate.
            for peer in peers:
                send(peer, neutral)
            apply_attempted = True
            owner = mapping.apply(service, saved)
            phase["owner"] = owner
            phase["afterApplyPositioning"] = ownership.position_receiver(service, receiver_path, identity,
                pointer_target=not args.keys_only, keyboard_window=target["window"] if args.keys_only else None)
            journal_scope = native.desktop_status(service)["runtime"]["backend"]["journalScope"]
            assert journal_scope == dict(kind="application", identity=identity), journal_scope
            phase["exactAppliedApplicationScope"] = journal_scope
            for peer in peers:
                send(peer, neutral)
            phase["neutralOwnersBeforePress"] = mapping.wait_for(lambda: neutral_owners(service, args.keys_only),
                "Fresh neutral controls did not establish empty native owners for the desktop routes")
            phase["warmupStartMarker"] = marker(service, 5 + index * 2)
            phase["warmup"] = warmup_keys(service, receiver_path, peers)
            phase["warmupEndMarker"] = marker(service, 6 + index * 2)
            phase["nativeSources"] = coherent_sources(service, configurations, peers)
            phase["uiBefore"] = control(service, "latency_ui_load", enabled=index == 1, profileName=saved["name"])
            report = native.receiver(receiver_path)
            if args.keys_only:
                assert report["foregroundWindow"] == target["window"], report
            else:
                phase["pointerTargetBeforeWorkload"] = report.get("pointerTarget")
                pointer = phase["pointerTargetBeforeWorkload"]
                assert pointer and pointer["exactReceiver"] and pointer["foregroundCapture"] == "0", pointer
            phase["startMarker"] = marker(service, 1 + index * 2)
            gate, abort = threading.Barrier(3), threading.Event()
            results, errors = [None, None], []
            threads = [threading.Thread(target=workload, args=(peer, position, gate, abort, results, errors), kwargs=dict(keys_only=args.keys_only), daemon=True)
                for position, peer in enumerate(peers)]
            for thread in threads: thread.start()
            gate.wait(timeout=5)
            for thread in threads: thread.join(timeout=SECONDS + 8)
            if any(thread.is_alive() for thread in threads):
                abort.set()
                for thread in threads: thread.join(timeout=4)
                raise AssertionError("A bounded WSS workload thread did not finish")
            assert not errors, errors
            phase["sources"] = results
            assert all(record["expectedImmediateKeyChanges"] >= 40 for record in results), "Insufficient immediate key samples"
            phase["settled"] = settle_keys(service, receiver_path, results,
                receiver_prefix=phase["warmup"]["receiverKeyPrefix"],
                pointer_baseline=None if args.keys_only else phase["warmup"]["pointerBaseline"])
            phase["endMarker"] = marker(service, 2 + index * 2)
            phase["uiAfter"] = control(service, "latency_ui_load", enabled=False, profileName=saved["name"])
            if index == 1:
                ui = phase["uiAfter"]
                assert ui["mode"] == "real-svelte-profiles-navigation" and ui["realProfilesView"] and ui["nativeCockpit"], ui
                assert ui["targetProfileVisible"] and ui["profileName"] == saved["name"], ui
                assert ui["navigationChanges"] >= 20, ui
            # Cleanup input is deliberately outside the phase marker interval.
            for peer in peers: send(peer, neutral)
            mapping.wait_for(lambda: native.desktop_status(service)["runtime"]["backend"]["held"] == [], "Neutral cleanup retained native keys")
            service.profiles("profile_stop")
            phase["stopProof"] = native.stopped_ownership(service, True)
            phase["auditCheckpoint"] = audit_checkpoint(service)
            phase["receiverAfterStop"] = native.receiver(receiver_path)
            assert len(phase["receiverAfterStop"].get("observations", [])) < 4096
            if args.keys_only:
                assert not any(event["message"] == 512 and event.get("injectedMarker") for event in phase["receiverAfterStop"]["observations"])
            if index == 0:
                companion.wait_native_companions(service, configurations, peers, previous=phase["nativeSources"])
        evidence["injected"] = True
        evidence["checks"] = ["Two authenticated WSS sources drove immediate KeyA/KeyB holds" + ("" if args.keys_only else " with normalized pointer load"),
            "Idle and real Svelte Profiles navigation phases retain exact accepted source sequences and changed-key receiver/native ownership counts between native audit markers",
            "The UI-load phase observes at least 20 transitions between native Overview and Profiles without shifting OS foreground to the WebView",
            "Every phase uses a fresh receiver, exact native scope and explicit Apply, followed by neutral cleanup, clean Stop and an acknowledged bounded audit checkpoint"]
    except Exception as error:
        evidence["failure"] = str(error)
        raise
    finally:
        evidence["nativeInputAttempted"] = apply_attempted
        evidence["injected"] |= receiver_injection_observed(receiver_paths)
        errors = []
        def attempt(label, action):
            try: return action()
            except Exception as error: errors.append(label + ": " + str(error))
        attempt("Stop UI load", lambda: control(service, "latency_ui_load", enabled=False, profileName=""))
        attempt("Stop native profile", lambda: service.profiles("profile_stop"))
        for peer in peers: attempt("Close test WSS peer", peer.close)
        proof = attempt("Prove clean native ownership", lambda: native.stopped_ownership(service, apply_attempted))
        evidence.update(ownershipCleanupConfirmed=proof is not None, needsRootCleanup=proof is None, finalOwnershipProof=proof)
        if proof is not None:
            if saved: attempt("Delete exact test profile", lambda: service.profiles("profile_delete", id=saved["id"], expectedRevision=saved["revision"]))
            for instance in assigned: attempt("Unassign exact test instance", lambda instance=instance: companion.operator(service, "companion_unassign", instanceId=instance))
            for instance in instances[:len(peers)]: attempt("Revoke exact test peer", lambda instance=instance: companion.operator(service, "companion_revoke", companionId=instance))
            if not listener_before: attempt("Restore disabled listener", lambda: companion.operator(service, "companion_disable"))
            evidence["finalAuditCheckpoint"] = attempt("Final audit checkpoint", lambda: audit_checkpoint(service))
        evidence["cleanupErrors"] = errors
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
        if errors and sys.exc_info()[0] is None: raise RuntimeError("; ".join(errors))
    print(json.dumps(dict(checks=evidence["checks"], phases=[dict(name=phase["name"], expectedKeyChanges=[r["expectedImmediateKeyChanges"] for r in phase["sources"]]) for phase in evidence["phases"]]), indent=2))


if __name__ == "__main__":
    main()
