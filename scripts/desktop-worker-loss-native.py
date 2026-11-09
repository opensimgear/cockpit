#!/usr/bin/env python3
"""Interrupt only the exact private desktop input helper in a disposable run."""
import argparse
import ctypes
import importlib.util
import json
import os
from pathlib import Path
import socket
import time
import uuid


def load(name):
    path = Path(__file__).with_name(name + ".py")
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


latency = load("desktop-latency-native")
crash = load("desktop-crash-native")
native, companion, mapping = latency.native, latency.companion, latency.mapping
KEY_A = {"kind": "key", "key": 4}


def workers(service):
    return latency.control(service, "latency_private_workers")


def persisted(service, owner):
    key = f"profile-desktop-{owner['session']}-{owner['generation']}.json"
    result = latency.control(service, "latency_recovery_record", key=key)
    assert result and result["revision"] > 0, "Missing exact desktop recovery record"
    return result


def journal_clean(record):
    state = record["payload"]
    assert state["held"] == state["possibleHeld"] == state["owners"] == state["runs"] == [], state
    assert state["cleanupComplete"] is True and state["uncertain"] is False and state["inFlight"] is None, state


def journal_scope(record, owner, identity):
    state = record["payload"]
    assert state["journalOwner"] == {"session": owner["session"], "generation": str(owner["generation"])}
    assert state["journalScope"] == {"kind": "application", "identity": identity}
    assert state["held"] in ([], [KEY_A]) and state["possibleHeld"] in ([], [KEY_A]), state
    return state


def stopped(service):
    state = service.profiles("profile_status")
    session = state["profileSession"]
    assert state["session"]["selectionMode"] == "manual"
    assert session["state"] == "stopped" and session["owner"] is None and session["appliedSnapshot"] is None, session
    assert not workers(service), "A native desktop helper is still running"
    return session


def key_edges(path):
    report = native.receiver(path)
    if "observations" not in report:
        return None
    events = [event for event in report["observations"] if event.get("injectedMarker") is True]
    assert all(event["message"] in (256, 257, 258) and event["scanCode"] == 30 and event["extended"] == 0 for event in events), events
    return [event["message"] for event in events if event["message"] != 258]


def wait_edges(path, expected):
    def matches():
        actual = key_edges(path)
        if actual is None:
            return None
        assert actual == expected[:len(actual)], (expected, actual)
        return actual if actual == expected else None
    return mapping.wait_for(matches, f"Exact receiver transitions did not reach {expected}")


def exact_foreground(windows, target):
    observed = windows.foreground()
    assert observed["window"] == int(target["window"]) and observed["identity"] == target["identity"], observed


def interrupt_worker(service, windows, app, target):
    rows = workers(service)
    assert len(rows) == 1, "Expected one live desktop worker"
    worker = windows.identity(rows[0]["pid"])
    assert worker and worker["pid"] not in (app["pid"], target["identity"]["pid"], os.getpid(), os.getppid())
    assert windows.path(worker["executable"]) == windows.path(app["executable"])
    assert int(worker["created"]) >= int(app["created"])
    handle = windows.open_exact(worker)
    try:
        assert windows.parents().get(worker["pid"]) == app["pid"] and windows.exact(app)
        assert windows.kernel.WaitForSingleObject(handle, 0) == 258
        exact_foreground(windows, target)
        requested = time.perf_counter_ns()
        assert windows.kernel.TerminateProcess(handle, 97), ctypes.get_last_error()
        assert windows.kernel.WaitForSingleObject(handle, 5000) == 0, "Private worker survived interruption"
        assert windows.exact(app) and windows.exact(target["identity"])
        return worker, requested, time.perf_counter_ns()
    finally:
        windows.kernel.CloseHandle(handle)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bridge", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--executable", required=True)
    parser.add_argument("--app-pid", type=int, required=True)
    parser.add_argument("--app-sha256", required=True)
    parser.add_argument("--inject", action="store_true")
    args = parser.parse_args()
    assert os.name == "nt" and args.inject
    assert all(os.environ.get(flag) == "1" for flag in ("OSG_DESKTOP_WORKER_LOSS_NATIVE", "OSG_DESKTOP_NATIVE_INJECT", "OSG_DESKTOP_LATENCY_NATIVE"))
    service, windows = native.NativeCockpit(args.bridge), crash.Windows()
    app = windows.identity(args.app_pid)
    assert app and windows.path(app["executable"]) == windows.path(args.executable)
    stopped(service)
    evidence = dict(nativeExecutableSha256=args.app_sha256, productionQualified=False, keysOnly=True,
        app=app, injected=False, workerLossVerified=False, cleanupProven=False, cleanupErrors=[])
    publication = latency.compiled_publication()
    instance = "desktop-helper-loss-" + uuid.uuid4().hex[:12]
    peer, target, path, owners = None, None, None, []
    try:
        if not companion.operator(service, "companion_status").get("listener", {}).get("enabled"):
            with socket.socket() as probe:
                probe.bind(("127.0.0.1", 0))
                port = probe.getsockname()[1]
            companion.operator(service, "companion_enable", address="127.0.0.1", advertisedHost="127.0.0.1", port=port, discovery=False)
        companion.publish(service, publication)
        peer = companion.Peer(service, instance)
        companion.operator(service, "companion_assign", companionId=instance, instanceId=instance, publicationHash=publication["publicationHash"])
        peer.open(instance, publication)
        document, assignments = latency.profile_for(service, [instance], publication, keys_only=True)
        saved = mapping.save(service, document, assignments)
        target = latency.control(service, "latency_receiver_begin", phase="idle")
        path, identity = Path(target["file"]), target["identity"]
        latency.ownership.position_receiver(service, path, identity, keyboard_window=target["window"])
        saved, _, _ = latency.capture_profile_scope(service, saved, assignments, identity)
        mapping.state(service, "profile_desktop_configure", "desktop", enabled=True, scope=dict(kind="application"))

        def send(value):
            latency.send(peer, [dict(controlId=latency.BUTTON, value=value)])

        def runtime():
            return native.desktop_status(service)["runtime"]

        def armed():
            state = runtime()
            return state["state"] == "active" and state["waitingNeutral"] == [] and state

        send(False)
        owner = mapping.apply(service, saved)
        owners.append(owner)
        latency.ownership.position_receiver(service, path, identity, keyboard_window=target["window"])
        send(False)
        mapping.wait_for(armed, "The exact scoped key route did not arm on neutral")
        assert key_edges(path) == []
        exact_foreground(windows, target)
        send(True)
        wait_edges(path, [256])
        evidence["injected"] = True

        def held_ack():
            state = runtime()
            backend = state["backend"]
            if backend["held"] != [KEY_A] or backend["inFlight"] is not None:
                return None
            outcomes = [entry["acknowledgement"] for entry in state["outcomes"]
                if entry["delivery"]["intent"] == {"kind": "held", "controls": [KEY_A]}]
            return outcomes[-1] if outcomes else None

        ack = mapping.wait_for(held_ack, "No committed native KeyA acknowledgement")
        assert ack["ok"] and ack["attempted"] == ack["confirmed"] == 1 and not ack["uncertain"], ack
        before = persisted(service, owner)
        initial = journal_scope(before, owner, identity)
        assert initial["held"] == [KEY_A] and initial["inFlight"] is None
        evidence.update(owner=owner, receiver=target, journalBefore=before, heldAcknowledgement=ack)

        worker, requested, exited = interrupt_worker(service, windows, app, target)
        evidence.update(interruptedWorker=worker, workerExitMs=(exited-requested)/1e6)

        def failed():
            state = service.profiles("profile_status")["profileSession"]
            return state if state["state"] == "recovery-needed" else None

        fault = mapping.wait_for(failed, "Live Cockpit did not report recovery after private helper exit")
        observed = time.perf_counter_ns()
        assert fault["owner"] is None and fault["fault"], fault
        assert runtime()["state"] == "recovery-needed"
        assert windows.exact(app) and key_edges(path) == [256] and workers(service) == []
        retained = persisted(service, owner)
        held = journal_scope(retained, owner, identity)
        assert held["held"] or held["possibleHeld"], held
        assert held["uncertain"] is True and held["cleanupComplete"] is False, held
        evidence.update(fault=fault, journalAfterLoss=retained, recoveryNeededMs=(observed-exited)/1e6)

        refused = service.request("profile_apply", id=saved["id"], expectedRevision=saved["revision"])
        assert refused["ok"] is False and failed() and not workers(service) and key_edges(path) == [256], refused
        evidence["applyBeforeRecoveryError"] = refused["error"]

        exact_foreground(windows, target)
        recovery = service.profiles("profile_recover")["profileSession"]
        wait_edges(path, [256, 257])
        stopped(service)
        assert recovery["state"] == "stopped" and runtime()["recovered"] is True
        recovered = persisted(service, owner)
        journal_clean(recovered)
        journal_scope(recovered, owner, identity)
        assert windows.exact(app)
        evidence.update(recovery=recovery, journalAfterRecovery=recovered)

        replacement = mapping.apply(service, saved)
        owners.append(replacement)
        assert replacement != owner
        mapping.wait_for(lambda: runtime()["state"] in ("active", "waiting-neutral"), "Replacement desktop route did not establish focus")
        assert key_edges(path) == [256, 257], "Apply replayed the old held input"
        send(False)
        mapping.wait_for(armed, "Replacement route did not admit a fresh neutral baseline")
        assert key_edges(path) == [256, 257], "A fresh baseline must not emit a key edge"
        current_workers = workers(service)
        assert len(current_workers) == 1
        new_worker = windows.identity(current_workers[0]["pid"])
        assert new_worker and new_worker != worker and windows.parents().get(new_worker["pid"]) == app["pid"]
        send(True)
        wait_edges(path, [256, 257, 256])
        send(False)
        wait_edges(path, [256, 257, 256, 257])
        service.profiles("profile_stop")
        stopped(service)
        journal_clean(persisted(service, replacement))
        evidence.update(replacementOwner=replacement, replacementWorker=new_worker,
            receiverEdges=key_edges(path), workerLossVerified=True)
    except Exception as error:
        evidence["failure"] = repr(error)
        raise
    finally:
        try:
            session = service.profiles("profile_status")["profileSession"]
            prior_edges = key_edges(path) if path else None
            if session["state"] == "recovery-needed" and target and owners:
                exact_foreground(windows, target)
                journal_scope(persisted(service, owners[-1]), owners[-1], target["identity"])
                service.profiles("profile_recover")
            elif session["state"] != "stopped":
                service.profiles("profile_stop")
            stopped(service)
            for run in owners:
                journal_clean(persisted(service, run))
            if path:
                if prior_edges and prior_edges[-1] == 256:
                    wait_edges(path, prior_edges + [257])
                observed = key_edges(path)
                assert observed is not None and (not observed or observed[-1] == 257), observed
            evidence["cleanupProven"] = True
        except Exception as error:
            evidence["cleanupErrors"].append(str(error))
        if peer:
            try:
                peer.close()
            except Exception as error:
                evidence["cleanupErrors"].append(str(error))
        args.output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
        assert not evidence["cleanupErrors"], evidence["cleanupErrors"]


if __name__ == "__main__":
    main()
