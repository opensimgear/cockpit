#!/usr/bin/env python3
"""Opt-in whole-development-app interruption and explicit owned KeyA recovery.

Only the applied native ProfileSession submits input. The receive-only window,
this harness and its CDP bridge remain outside the interrupted application tree.
"""
import argparse
import copy
import ctypes
from ctypes import wintypes
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("desktop_game", ROOT / "scripts/desktop-game-native.py")
game = importlib.util.module_from_spec(spec)
spec.loader.exec_module(game)
native, mapping, companion = game.native, game.mapping, game.companion
KEY_A = {"kind": "key", "key": 4}


class ProcessEntry(ctypes.Structure):
    _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD), ("th32DefaultHeapID", ctypes.c_size_t),
        ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD),
        ("th32ParentProcessID", wintypes.DWORD), ("pcPriClassBase", wintypes.LONG),
        ("dwFlags", wintypes.DWORD), ("szExeFile", wintypes.WCHAR * 260)]


class Windows(game.Windows):
    """Terminate handles pinned to one verified development application tree."""
    def __init__(self):
        super().__init__()
        self.interrupted = False
        for function, args, result in [
            (self.kernel.CreateToolhelp32Snapshot, [wintypes.DWORD, wintypes.DWORD], wintypes.HANDLE),
            (self.kernel.Process32FirstW, [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)], wintypes.BOOL),
            (self.kernel.Process32NextW, [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)], wintypes.BOOL),
            (self.kernel.TerminateProcess, [wintypes.HANDLE, wintypes.UINT], wintypes.BOOL)]:
            function.argtypes, function.restype = args, result

    def parents(self):
        snapshot = self.kernel.CreateToolhelp32Snapshot(2, 0)
        assert snapshot not in (None, ctypes.c_void_p(-1).value), ctypes.get_last_error()
        try:
            entry, result = ProcessEntry(), {}
            entry.dwSize = ctypes.sizeof(entry)
            success = self.kernel.Process32FirstW(snapshot, ctypes.byref(entry))
            while success:
                result[entry.th32ProcessID] = entry.th32ParentProcessID
                success = self.kernel.Process32NextW(snapshot, ctypes.byref(entry))
            assert len(result) < 65536, "Unbounded process snapshot"
            return result
        finally:
            self.kernel.CloseHandle(snapshot)

    def open_exact(self, identity):
        handle = self.kernel.OpenProcess(0x1000 | 0x100000 | 1, False, identity["pid"])
        assert handle, "Cannot pin owned process for interruption"
        try:
            assert self.kernel.WaitForSingleObject(handle, 0) == 258, "Owned process exited before interruption"
            buffer, size = ctypes.create_unicode_buffer(32768), wintypes.DWORD(32768)
            assert self.kernel.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size))
            created, exited, kernel, user = (wintypes.FILETIME() for _ in range(4))
            assert self.kernel.GetProcessTimes(handle, *map(ctypes.byref, [created, exited, kernel, user]))
            assert str((created.dwHighDateTime << 32) | created.dwLowDateTime) == identity["created"]
            assert self.path(buffer.value) == self.path(identity["executable"])
            return handle
        except BaseException:
            self.kernel.CloseHandle(handle)
            raise

    def interrupt(self, root, executable, receiver_pid):
        assert self.path(root["executable"]) == self.path(executable) and self.exact(root)
        parents, owned = self.parents(), {root["pid"]: root}
        while True:
            additions = {}
            for pid, parent in parents.items():
                if parent in owned and pid not in owned:
                    identity = self.identity(pid)
                    if identity:
                        assert int(identity["created"]) >= int(owned[parent]["created"]), "Recycled parent PID"
                        additions[pid] = dict(identity, parentPid=parent)
            if not additions:
                break
            owned.update(additions)
            assert len(owned) <= 40, "Development application tree exceeds acceptance bound"
        assert receiver_pid not in owned and os.getpid() not in owned and os.getppid() not in owned
        workers = [item for pid, item in owned.items() if pid != root["pid"] and self.path(item["executable"]) == self.path(executable)]
        assert len(workers) == 1 and workers[0]["parentPid"] == root["pid"], "Expected one private native worker child"
        console = self.path(str(Path(os.environ["SystemRoot"]) / "System32/conhost.exe"))
        webview = self.path(str(Path(os.environ["ProgramFiles(x86)"]) / "Microsoft/EdgeWebView/Application")) + os.sep
        for item in owned.values():
            path = self.path(item["executable"])
            known_webview = path.startswith(webview) and Path(path).name == "msedgewebview2.exe"
            known_console = path == console and item.get("parentPid") in (root["pid"], workers[0]["pid"])
            assert path == self.path(executable) or known_webview or known_console, "Unknown application descendant preserved: " + item["executable"]
        handles = {}
        try:
            for pid, item in owned.items():
                handles[pid] = self.open_exact(item)
            # All process objects are now pinned. Re-read ancestry so a PID
            # recycled between the initial snapshot and identity lookup cannot
            # authorize interruption of an unrelated process.
            pinned_parents = self.parents()
            for pid, item in owned.items():
                assert self.kernel.WaitForSingleObject(handles[pid], 0) == 258, "Owned process exited during verification"
                if pid != root["pid"]:
                    assert pinned_parents.get(pid) == item["parentPid"], "Application descendant ancestry changed"
            started = time.perf_counter_ns()
            # Kill the only input worker before its parent can close the pipe and
            # trigger graceful worker cleanup. No destructor or replay is run.
            self.interrupted = True
            assert self.kernel.TerminateProcess(handles[workers[0]["pid"]], 97), ctypes.get_last_error()
            assert self.kernel.TerminateProcess(handles[root["pid"]], 97), ctypes.get_last_error()
            parent_terminated = time.perf_counter_ns()
            for pid in reversed(list(owned)):
                if pid not in (root["pid"], workers[0]["pid"]) and self.kernel.WaitForSingleObject(handles[pid], 0) == 258:
                    assert self.kernel.TerminateProcess(handles[pid], 97), ctypes.get_last_error()
            for handle in handles.values():
                assert self.kernel.WaitForSingleObject(handle, 5000) == 0, "Owned application process survived termination"
            assert not self.matching(executable), "Development native app or input worker survived interruption"
            return dict(tree=list(owned.values()), worker=workers[0], method="TerminateProcess on exact opened identity handles; private worker then main application then its snapshotted WebView descendants", workerToParentTerminationNs=str(parent_terminated - started))
        finally:
            for handle in handles.values():
                self.kernel.CloseHandle(handle)


class NativeCockpit(native.NativeCockpit):
    def request(self, command, **fields):
        request = urllib.request.Request(self.endpoint, json.dumps(dict(command=command, **fields)).encode(),
            {"Content-Type": "application/json", "Authorization": "Bearer " + os.environ["OSG_NATIVE_BRIDGE_TOKEN"]})
        with urllib.request.urlopen(request, timeout=75) as response:
            result = response.read(2 * 1024 * 1024 + 1)
        assert len(result) <= 2 * 1024 * 1024
        return json.loads(result)


def desktop(service):
    return mapping.state(service, "profile_desktop_status", "desktop")


def key_events(path, start=0):
    events = native.injected_events(path)[start:]
    # TranslateMessage can derive WM_CHAR from the injected KeyA message. It is
    # not a second native submission; preserve it in raw receiver evidence.
    assert all(event["message"] in (256, 257, 258) and event.get("scanCode") == 30 and event.get("extended") == 0 for event in events), "Unexpected injected control in bounded KeyA acceptance"
    return [event for event in events if event["message"] in (256, 257)]


def profile_for(service, instance, publication):
    document, assignments = game.profile_for(service, instance, publication)
    document["name"] = "Actual crash KeyA recovery " + uuid.uuid4().hex[:8]
    document["desktopOutputs"][0]["name"] = "One KeyA hold in receive-only target"
    document["desktopOutputs"][0]["actions"] = [dict(portId="key", action=dict(kind="key-hold", key=4))]
    return document, assignments


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bridge", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--receiver", type=Path, required=True)
    parser.add_argument("--interrupt", action="store_true")
    args = parser.parse_args()
    assert args.interrupt and os.environ.get("OSG_DESKTOP_CRASH_NATIVE") == "1" and os.environ.get("OSG_DESKTOP_NATIVE_INJECT") == "1", "Whole-app interruption and native injection require both opt-ins and --interrupt"
    assert os.name == "nt" and not os.environ.get("OSG_DCS_BIOS_CONFIG"), "Only default native Windows config is supported"
    service, windows = NativeCockpit(args.bridge), Windows()
    executable = str(ROOT / "src-tauri/target/debug/osg-cockpit.exe")
    assert Path(executable).is_file()
    applications = windows.matching(executable)
    assert len(applications) == 1, "Expected exactly one running workspace dev app and no desktop input workers"
    app_before = applications[0]
    settings = Path(os.environ["APPDATA"]) / "OpenSimGear/Cockpit/desktop-input-v1.json"
    settings_before = settings.read_bytes() if settings.exists() else None
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if settings_before is not None:
        args.output.with_suffix(".settings-before.bin").write_bytes(settings_before)
    before, profile_before = desktop(service), service.profiles("profile_status")["profileSession"]
    assert before["qualified"] and not before["enabled"] and profile_before["state"] == "stopped" and profile_before["owner"] is None
    publication = json.loads((ROOT / "crates/companion-service/tests/fixtures/c1-flight-publication.json").read_text())
    evidence = dict(actualNativeCockpit=True, actualWholeAppInterruption=False, injected=False, explicitRecovery=False,
        virtualControllers=0, productionQualified=False, hardwareTested=False, gameTested=False,
        measurement="Native ACK counts, exact Win32 process ownership, durable journal and receiver-observed injected KeyA edges. No latency or hardware claim.", checks=[], desktopBefore=before, appBefore=app_before)
    peer, interrupted, recovered, listener_enabled = None, False, False, None
    instance = "desktop-crash-" + uuid.uuid4().hex[:12]
    event_start = len(native.injected_events(args.receiver))
    try:
        listener_before = companion.operator(service, "companion_status").get("listener", {})
        listener_enabled = listener_before.get("enabled", False)
        evidence["listenerBefore"] = copy.deepcopy(listener_before)
        if not listener_enabled:
            with socket.socket() as probe:
                probe.bind(("127.0.0.1", 0))
                port = probe.getsockname()[1]
            companion.operator(service, "companion_enable", address="127.0.0.1", advertisedHost="127.0.0.1", port=port, discovery=False)
        companion.publish(service, publication)
        peer = companion.Peer(service, instance)
        companion.operator(service, "companion_assign", companionId=instance, instanceId=instance, publicationHash=publication["publicationHash"])
        peer.open(instance, publication)
        document, assignments = profile_for(service, instance, publication)
        saved = mapping.save(service, document, assignments)
        compiled = mapping.state(service, "profile_compile", "profileCompilation", document=saved, assignments=assignments)
        assert compiled["readiness"] != "invalid" and compiled["requirements"] == dict(desktopInput=True, virtualDevices=False, visibility=False)
        assert compiled["budgets"]["virtualTargets"] == 0
        service.profiles("profile_select", id=saved["id"], selectionSource="manual")
        service.profiles("profile_selection_mode", mode="manual")
        receiver_identity = mapping.wait_for(lambda: native.receiver(args.receiver).get("identity"), "Receive-only target is not ready")
        assert windows.exact(receiver_identity) and receiver_identity["pid"] not in (app_before["pid"], os.getpid(), os.getppid())
        args.receiver.with_suffix(".focus").write_text("focus")
        mapping.wait_for(lambda: desktop(service).get("recentApplication", {}).get("pid") == receiver_identity["pid"], "Native foreground observer did not see receiver")
        captured = mapping.state(service, "profile_desktop_capture_application", "desktop")
        # The settings response uses the existing game identity shape (path,
        # pid, numeric epoch); the runtime scope/journal uses lossless created.
        assert captured["application"]["pid"] == receiver_identity["pid"] and windows.path(captured["application"]["path"]) == windows.path(receiver_identity["executable"])
        assert captured["recentApplication"] == receiver_identity
        mapping.state(service, "profile_desktop_configure", "desktop", enabled=True, scope=dict(kind="application"))
        service.profiles("profile_pause", paused=False)
        owner = mapping.apply(service, saved)
        peer.input([dict(controlId="flight-input-0", value=False)])
        mapping.wait_for(lambda: desktop(service)["runtime"]["state"] == "active" and not desktop(service)["runtime"]["waitingNeutral"], "Neutral scoped desktop route did not arm")
        assert key_events(args.receiver, event_start) == []
        peer.input([dict(controlId="flight-input-0", value=True)])
        def held():
            status = desktop(service)
            outcomes = [outcome for outcome in status["runtime"].get("outcomes", []) if outcome["delivery"]["intent"] == {"kind": "held", "controls": [KEY_A]} and outcome["acknowledgement"]["attempted"] > 0]
            return (status, outcomes[-1]["acknowledgement"]) if status["runtime"]["backend"]["held"] == [KEY_A] and outcomes else None
        held_status, ack = mapping.wait_for(held, "No acknowledged native KeyA hold")
        assert ack["ok"] and ack["attempted"] == ack["confirmed"] == 1 and not ack["uncertain"] and not ack["skipped"]
        mapping.wait_for(lambda: len(key_events(args.receiver, event_start)) == 1 and key_events(args.receiver, event_start)[0]["message"] == 256, "Receiver did not observe exactly one KeyA down")
        journal = settings.with_name("profile-desktop-" + owner["session"] + "-" + str(owner["generation"]) + ".json")
        journal_before = json.loads(journal.read_text())
        assert journal_before["held"] == [KEY_A] and journal_before["journalOwner"] == {"session": owner["session"], "generation": str(owner["generation"])}
        assert journal_before["journalScope"] == {"kind": "application", "identity": receiver_identity}
        evidence.update(appliedOwner=owner, heldAcknowledgement=ack, receiverIdentity=receiver_identity, journalBefore=journal_before, profile=saved, assignments=assignments)
        evidence["injected"] = True
        evidence["checks"].append("One authenticated real WSS button produced exactly one receiver-observed and natively acknowledged application-scoped KeyA hold with durable exact run ownership")
        # No live peer request is sent after this boundary. Socket EOF after the
        # worker and app are gone cannot cause graceful input cleanup.
        evidence["interruption"] = windows.interrupt(app_before, executable, receiver_identity["pid"])
        interrupted = True
        evidence["actualWholeAppInterruption"] = True
        evidence["journalAfterInterruption"] = json.loads(journal.read_text())
        assert evidence["journalAfterInterruption"]["held"] == [KEY_A]
        assert windows.exact(receiver_identity) and key_events(args.receiver, event_start)[-1]["message"] == 256
        evidence["checks"].append("Exact private worker, main dev app and owned WebView descendants were forcibly terminated without Drop; the receiver and external harness survived and held KeyA remained durably recorded")
        # Restart the same development executable, with the same explicit debug
        # qualification and CDP setting. Vite is a separate existing dev server.
        env = dict(os.environ, OSG_DEV_DESKTOP_INPUT="1", WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222")
        env.pop("OSG_DCS_BIOS_CONFIG", None)
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = 0
        with args.output.with_suffix(".restart.log").open("wb") as log:
            restarted = subprocess.Popen([str(executable)], cwd=ROOT, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, startupinfo=startup, creationflags=subprocess.CREATE_NO_WINDOW)
        restart_identity = mapping.wait_for(lambda: windows.identity(restarted.pid), "Restarted native dev app has no exact identity")
        assert windows.path(restart_identity["executable"]) == windows.path(executable) and restart_identity != app_before
        evidence["appAfter"] = restart_identity
        restart_status = service.profiles("profile_status")["profileSession"]
        restart_desktop = desktop(service)
        assert restart_status["state"] == "recovery-needed" and restart_status["owner"] is None and restart_status["appliedSnapshot"] is None
        assert restart_desktop["runtime"]["state"] == "recovery-needed"
        time.sleep(.3)
        assert len(windows.matching(executable)) == 1, "Ordinary startup spawned an input worker"
        assert len(key_events(args.receiver, event_start)) == 1, "Ordinary startup released or replayed KeyA"
        evidence["ordinaryRestart"] = dict(profileSession=restart_status, desktop=restart_desktop, journal=json.loads(journal.read_text()), receiverEvents=key_events(args.receiver, event_start))
        evidence["checks"].append("Ordinary whole-app restart exposes RecoveryNeeded with no owner, applied snapshot, input worker, release or replay")
        args.receiver.with_suffix(".focus").write_text("focus")
        mapping.wait_for(lambda: windows.foreground().get("identity", {}).get("pid") == receiver_identity["pid"], "Cannot focus exact surviving receiver for recorded recovery")
        result = service.profiles("profile_recover")["profileSession"]
        mapping.wait_for(lambda: len(key_events(args.receiver, event_start)) == 2 and key_events(args.receiver, event_start)[1]["message"] == 257, "Explicit recovery did not release only recorded KeyA")
        journal_after = json.loads(journal.read_text())
        assert result["state"] == "stopped" and result["owner"] is None and result["appliedSnapshot"] is None
        assert journal_after["held"] == [] and journal_after["possibleHeld"] == [] and journal_after["inFlight"] is None
        assert journal_after["cleanupComplete"] and not journal_after["uncertain"] and journal_after["owners"] == [] and journal_after["runs"] == []
        assert journal_after["journalOwner"] == journal_before["journalOwner"] and journal_after["journalScope"] == journal_before["journalScope"]
        time.sleep(.2)
        assert len(key_events(args.receiver, event_start)) == 2 and len(windows.matching(executable)) == 1
        recovered = True
        evidence.update(explicitRecovery=True, recovery=dict(profileSession=result, desktop=desktop(service), journal=journal_after), receiverEvents=key_events(args.receiver, event_start), receiverRaw=native.receiver(args.receiver))
        evidence["checks"].append("Explicit profile recovery releases exactly recorded KeyA once, preserves journal owner/scope, clears held/possible/in-flight ownership, leaves ProfileSession stopped and terminates its recovery worker")
    except Exception as error:
        interrupted = windows.interrupted
        evidence["actualWholeAppInterruption"] = interrupted
        evidence["failure"] = repr(error)
        evidence["needsRootCleanup"] = interrupted and not recovered
        raise
    finally:
        errors = []
        def cleanup(label, operation):
            try:
                return operation()
            except Exception as error:
                errors.append(label + ": " + str(error))
        if not interrupted or recovered:
            cleanup("Stop native profile", lambda: service.profiles("profile_stop"))
            cleanup("Disable output and restore scope", lambda: mapping.state(service, "profile_desktop_configure", "desktop", enabled=False, scope=before["scope"]))
            def restore_settings():
                if settings_before is None:
                    settings.unlink(missing_ok=True)
                else:
                    settings.write_bytes(settings_before)
            cleanup("Restore exact native desktop preference file", restore_settings)
            if listener_enabled is False:
                cleanup("Restore disabled listener", lambda: companion.operator(service, "companion_disable"))
            cleanup("Revoke only test companion", lambda: companion.operator(service, "companion_revoke", companionId=instance))
            def verify():
                run, status = service.profiles("profile_status")["profileSession"], desktop(service)
                listener = companion.operator(service, "companion_status").get("listener", {})
                apps = windows.matching(executable)
                assert (settings.read_bytes() if settings.exists() else None) == settings_before, "Native desktop preferences were not restored byte for byte"
                assert run["state"] == "stopped" and run["owner"] is None and not status["enabled"]
                assert status["application"] == before["application"] and status["scope"] == before["scope"]
                assert listener.get("enabled", False) == listener_enabled
                assert len(apps) == 1 and windows.exact(evidence.get("appAfter", app_before))
                evidence["finalStatus"] = dict(profileSession=run, desktop=native.compact_desktop(status), listener=listener, nativeApps=apps)
            cleanup("Verify stopped restored dev app without input workers", verify)
        # A failed explicit recovery deliberately retains the owned journal,
        # enabled recovery settings and receive-only target for root coordination.
        if peer:
            cleanup("Close original WSS peer", peer.close)
        evidence["cleanupErrors"] = errors
        evidence["selectionAfterTest"] = "Manual test profile retained, stopped; no automatic reactivation."
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
        if errors and sys.exc_info()[0] is None:
            raise RuntimeError("; ".join(errors))
    print(json.dumps(dict(checks=evidence["checks"], explicitRecovery=evidence["explicitRecovery"], appAfter=evidence["appAfter"]), indent=2))


if __name__ == "__main__":
    main()
