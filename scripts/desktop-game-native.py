#!/usr/bin/env python3
"""Opt-in actual Steam game launch through the already-running native Cockpit.

This observes exact native ownership and SendInput acknowledgements. It does not
infer that the game consumed an input or that its displayed state changed.
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
import sys
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("desktop_native", ROOT / "scripts/desktop-companion-native.py")
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)
companion, mapping = native.companion, native.mapping
GAME = "assetto-corsa-rally"
RIGHT = {"kind": "key", "key": 79}


class NativeCockpit(native.NativeCockpit):
    def request(self, command, **fields):
        # Native startup has a bounded 45-second process observation window.
        request = urllib.request.Request(self.endpoint, json.dumps(dict(command=command, **fields)).encode(),
            {"Content-Type": "application/json", "Authorization": "Bearer " + os.environ["OSG_NATIVE_BRIDGE_TOKEN"]})
        with urllib.request.urlopen(request, timeout=120 if command == "profile_game_launch" else 30) as response:
            result = response.read(2 * 1024 * 1024 + 1)
        assert len(result) <= 2 * 1024 * 1024
        return json.loads(result)


class Windows:
    """Read process identities; focus and close only one verified task process."""
    def __init__(self):
        assert os.name == "nt", "Actual game acceptance requires Windows"
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.user = ctypes.WinDLL("user32", use_last_error=True)
        self.psapi = ctypes.WinDLL("psapi", use_last_error=True)
        self.callback = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        signatures = [
            (self.kernel.OpenProcess, [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.HANDLE),
            (self.kernel.CloseHandle, [wintypes.HANDLE], wintypes.BOOL),
            (self.kernel.QueryFullProcessImageNameW, [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)], wintypes.BOOL),
            (self.kernel.GetProcessTimes, [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4, wintypes.BOOL),
            (self.kernel.WaitForSingleObject, [wintypes.HANDLE, wintypes.DWORD], wintypes.DWORD),
            (self.kernel.GetCurrentThreadId, [], wintypes.DWORD),
            (self.psapi.EnumProcesses, [ctypes.POINTER(wintypes.DWORD), wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)], wintypes.BOOL),
            (self.user.EnumWindows, [self.callback, wintypes.LPARAM], wintypes.BOOL),
            (self.user.GetWindowThreadProcessId, [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)], wintypes.DWORD),
            (self.user.GetForegroundWindow, [], wintypes.HWND),
            (self.user.IsWindowVisible, [wintypes.HWND], wintypes.BOOL),
            (self.user.GetClientRect, [wintypes.HWND, ctypes.POINTER(wintypes.RECT)], wintypes.BOOL),
            (self.user.GetWindowTextW, [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int], ctypes.c_int),
            (self.user.ShowWindow, [wintypes.HWND, ctypes.c_int], wintypes.BOOL),
            (self.user.BringWindowToTop, [wintypes.HWND], wintypes.BOOL),
            (self.user.SetForegroundWindow, [wintypes.HWND], wintypes.BOOL),
            (self.user.AttachThreadInput, [wintypes.DWORD, wintypes.DWORD, wintypes.BOOL], wintypes.BOOL),
            (self.user.PostMessageW, [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM], wintypes.BOOL),
        ]
        for function, args, result in signatures:
            function.argtypes, function.restype = args, result

    @staticmethod
    def path(value):
        return os.path.normcase(os.path.realpath(value.removeprefix("\\\\?\\")))

    def identity(self, pid):
        handle = self.kernel.OpenProcess(0x1000 | 0x100000, False, pid)
        if not handle:
            return None
        try:
            if self.kernel.WaitForSingleObject(handle, 0) != 258:
                return None
            buffer, size = ctypes.create_unicode_buffer(32768), wintypes.DWORD(32768)
            if not self.kernel.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
                return None
            created, exited, kernel, user = (wintypes.FILETIME() for _ in range(4))
            if not self.kernel.GetProcessTimes(handle, *map(ctypes.byref, [created, exited, kernel, user])):
                return None
            return dict(executable=buffer.value, pid=pid, created=str((created.dwHighDateTime << 32) | created.dwLowDateTime))
        finally:
            self.kernel.CloseHandle(handle)

    def exact(self, expected):
        current = self.identity(expected["pid"])
        return bool(current and current["created"] == expected["created"] and self.path(current["executable"]) == self.path(expected["executable"]))

    def matching(self, executable):
        pids, used = (wintypes.DWORD * 65536)(), wintypes.DWORD()
        assert self.psapi.EnumProcesses(pids, ctypes.sizeof(pids), ctypes.byref(used)), ctypes.get_last_error()
        assert used.value < ctypes.sizeof(pids), "Process enumeration exceeded its bound"
        result = []
        for pid in pids[:used.value // ctypes.sizeof(wintypes.DWORD)]:
            value = self.identity(pid)
            if value and self.path(value["executable"]) == self.path(executable):
                result.append(value)
        return result

    def foreground(self):
        pid = wintypes.DWORD()
        hwnd = self.user.GetForegroundWindow()
        self.user.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        return dict(window=int(hwnd or 0), identity=self.identity(pid.value))

    def owns_window(self, identity, hwnd):
        pid = wintypes.DWORD()
        self.user.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        return pid.value == identity["pid"] and self.exact(identity)

    def windows(self, identity):
        assert self.exact(identity), "The verified task game process changed or exited"
        result = []
        def visit(hwnd, _):
            pid, rect = wintypes.DWORD(), wintypes.RECT()
            self.user.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value == identity["pid"] and self.user.IsWindowVisible(hwnd) and self.user.GetClientRect(hwnd, ctypes.byref(rect)):
                title = ctypes.create_unicode_buffer(512)
                self.user.GetWindowTextW(hwnd, title, len(title))
                result.append(dict(window=int(hwnd), width=rect.right, height=rect.bottom, title=title.value))
            return True
        assert self.user.EnumWindows(self.callback(visit), 0), ctypes.get_last_error()
        return sorted(result, key=lambda item: item["width"] * item["height"], reverse=True)

    def focus(self, identity, known_window=None):
        windows = self.windows(identity) if known_window is None else [known_window]
        if not windows:
            return None
        if known_window is None and (windows[0]["width"] < 640 or windows[0]["height"] < 360):
            return None
        hwnd = windows[0]["window"]
        if not self.owns_window(identity, hwnd):
            return None
        current_thread = self.kernel.GetCurrentThreadId()
        threads = {self.user.GetWindowThreadProcessId(self.user.GetForegroundWindow(), None), self.user.GetWindowThreadProcessId(hwnd, None)} - {0, current_thread}
        attached = [thread for thread in threads if self.user.AttachThreadInput(current_thread, thread, True)]
        try:
            # Exclusive/fullscreen games can minimize on focus loss; restore the
            # previously verified HWND before consulting its client geometry.
            self.user.ShowWindow(hwnd, 9)
            self.user.BringWindowToTop(hwnd)
            self.user.SetForegroundWindow(hwnd)
        finally:
            for thread in attached:
                self.user.AttachThreadInput(current_thread, thread, False)
        foreground = self.foreground()
        return dict(window=windows[0], foreground=foreground) if foreground["identity"] and foreground["identity"]["pid"] == identity["pid"] and self.exact(identity) else None

    def close_task_game(self, identity):
        if not self.exact(identity):
            return dict(state="already-exited", identity=identity)
        windows = self.windows(identity)
        for window in windows:
            assert self.owns_window(identity, window["window"]), "Game window or process identity changed before graceful close"
            assert self.user.PostMessageW(window["window"], 0x0010, 0, 0), ctypes.get_last_error()
        deadline = time.monotonic() + 15
        while self.exact(identity) and time.monotonic() < deadline:
            time.sleep(.1)
        return dict(state="left-running" if self.exact(identity) else "closed", identity=identity,
            method="WM_CLOSE to visible windows of the exact newly launched process; no forced termination")


def profile_for(service, instance, publication):
    detail = mapping.wait_for(lambda: companion.selected_resource(service, instance), "No exact current WSS companion ports")
    observed = next(port for port in detail["ports"] if port["descriptor"]["id"] == "flight-input-0")
    contract = observed["descriptor"]["contract"]
    assert observed["current"] and observed["descriptor"]["direction"] == "output"
    assert contract["delivery"] == "state" and contract["schema"] == {"type": "boolean"}
    document = mapping.blank("Actual ACR desktop launch " + uuid.uuid4().hex[:8])
    document["resources"] = [dict(id="panel", name="Authenticated real WSS button", kind="companion", required=False,
        ports=[mapping.port("button", contract, "output")])]
    target_contract = dict(delivery="state", schema=dict(type="boolean"), freshnessMs=1000, fallback=False)
    document["desktopOutputs"] = [dict(id="desktop", name="Right Arrow in launched game", required=True,
        ports=[mapping.port("key", target_contract, "input")], actions=[dict(portId="key", action=dict(kind="key-hold", key=79))])]
    document["bindings"] = [dict(id="route-key", **{"from": dict(nodeId="panel", portId="button"), "to": dict(nodeId="desktop", portId="key")})]
    document["companionAssignments"] = [dict(resourceId="panel", publicationHash=publication["publicationHash"])]
    assignments = [dict(resourceId="panel", sourceId=detail["resource"]["id"], ports=[dict(profilePortId="button",
        observedPortId="flight-input-0", expectedContractHash=observed["contractHash"])])]
    return document, assignments


def desktop(service):
    return mapping.state(service, "profile_desktop_status", "desktop")


def press_outcomes(status):
    return [outcome for outcome in (status.get("runtime") or {}).get("outcomes", [])
        if outcome["delivery"]["intent"] == {"kind": "held", "controls": [RIGHT]}
        and outcome["acknowledgement"]["attempted"] > 0]


def clean(status, state=None):
    runtime = status.get("runtime") or {}
    backend = runtime.get("backend") or {}
    return (state is None or runtime.get("state") == state) and backend.get("held") == [] and backend.get("possibleHeld") == [] and backend.get("cleanupComplete") and not backend.get("uncertain")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bridge", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--receiver", type=Path, required=True)
    parser.add_argument("--desktop-settings-file", type=Path, required=True)
    parser.add_argument("--launch", action="store_true", help="Explicitly launch the installed game through native ProfileSession")
    args = parser.parse_args()
    assert args.launch and os.environ.get("OSG_DESKTOP_GAME_NATIVE") == "1" and os.environ.get("OSG_DESKTOP_NATIVE_INJECT") == "1", "Actual game launch and native injection require both opt-ins and --launch"
    service, windows = NativeCockpit(args.bridge), Windows()
    evidence = dict(actualNativeCockpit=True, actualGame=GAME, actualLaunchRequested=False, injected=False,
        productionQualified=False, virtualControllers=0, observedGameBehavior=False,
        measurement="Native process/foreground identity, typed ownership and SendInput counts; game consumption and displayed response are not measured.", checks=[])
    before = desktop(service)
    assert before["qualified"], "The running Cockpit is not an explicit desktop qualification build"
    assert service.profiles("profile_status")["profileSession"]["owner"] is None, "Stop the existing profile before this isolated acceptance"
    games = mapping.state(service, "game_get_state", "games")
    game = next(item for item in games["catalogue"] if item["id"] == GAME)
    assert game["installed"] and game["launchSupported"] and Path(game["executable"]).is_file(), game
    assert not windows.matching(game["executable"]), "A matching game was already running; the harness only owns newly launched processes"
    settings_existed = args.desktop_settings_file.exists()
    settings_before = args.desktop_settings_file.read_bytes() if settings_existed else None
    evidence.update(installation=game, desktopBefore=before, automaticBefore=service.profiles("profile_automatic_status")["automatic"])
    publication = json.loads((ROOT / "scripts/fixtures/compiled-flight-publication.json").read_text())
    instance, peer, identity, listener_enabled = "desktop-game-" + uuid.uuid4().hex[:12], None, None, None
    try:
        listener_enabled = companion.operator(service, "companion_status").get("listener", {}).get("enabled", False)
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
        compilation = mapping.state(service, "profile_compile", "profileCompilation", document=saved, assignments=assignments)
        assert compilation["readiness"] != "invalid", compilation
        assert compilation["requirements"] == dict(desktopInput=True, virtualDevices=False, visibility=False), compilation
        assert compilation["budgets"]["virtualTargets"] == 0
        evidence.update(profile=saved, assignments=assignments, compilation=compilation)
        mapping.state(service, "profile_desktop_configure", "desktop", enabled=True, scope=dict(kind="application"))
        service.profiles("profile_select", id=saved["id"], selectionSource="manual")
        service.profiles("profile_selection_mode", mode="manual")
        service.profiles("profile_stop")
        selected = service.profiles("profile_automatic_status")["automatic"]
        assert selected["mode"] == "manual" and selected["manualProfileId"] == saved["id"], selected
        evidence["actualLaunchRequested"] = True
        launched = service.profiles("profile_game_launch", gameId=GAME)
        evidence["launchResult"] = launched
        assert launched["launch"]["state"] == "started" and launched["launch"]["profileId"] == saved["id"], launched
        run = launched["profileSession"]
        assert run["state"] in ["running", "degraded"] and run["owner"]["profileId"] == saved["id"] and run["owner"]["profileRevision"] == saved["revision"], run
        status = desktop(service)
        assert status["runtime"]["scope"]["kind"] == "application", status
        identity = status["runtime"]["scope"]["identity"]
        assert windows.path(identity["executable"]) == windows.path(game["executable"]) and windows.exact(identity), identity
        assert status["runtime"]["backend"]["journalScope"] == status["runtime"]["scope"]
        assert status["runtime"]["backend"]["journalOwner"]["session"] == run["owner"]["session"]
        assert int(status["runtime"]["backend"]["journalOwner"]["generation"]) == run["owner"]["generation"]
        evidence.update(actualProcess=identity, appliedOwner=run["owner"], appliedSnapshot=run["appliedSnapshot"])
        evidence["checks"].append("Manual saved profile launches the installed Steam game and binds its exact PID/executable/creation identity into the owned desktop journal")
        evidence["gameFocus"] = mapping.wait_for(lambda: windows.focus(identity), "Verified game did not expose a focusable main window", seconds=90)
        def focused():
            runtime = desktop(service)["runtime"]
            expected = "waiting" if runtime["waitingNeutral"] else "active"
            return runtime["state"] == expected
        mapping.wait_for(focused, "Actual game foreground did not establish an eligible application scope")
        peer.input([dict(controlId="flight-input-0", value=False)])
        mapping.wait_for(lambda: "route-key" not in desktop(service)["runtime"]["waitingNeutral"], "Fresh game baseline did not arm the route")
        initial_presses = len(press_outcomes(desktop(service)))
        peer.input([dict(controlId="flight-input-0", value=True)])
        def held():
            status = desktop(service)
            return status if status["runtime"]["state"] == "active" and status["runtime"]["backend"]["held"] == [RIGHT] and len(press_outcomes(status)) == initial_presses + 1 else None
        held_status = mapping.wait_for(held, "Real WSS button did not produce an acknowledged game-scoped Right Arrow hold")
        ack = press_outcomes(held_status)[-1]["acknowledgement"]
        assert ack["ok"] and ack["attempted"] == ack["confirmed"] == 1 and not ack["uncertain"] and not ack["skipped"], ack
        assert windows.foreground()["identity"] == windows.identity(identity["pid"])
        assert windows.foreground()["identity"]["pid"] == identity["pid"]
        evidence["gameHold"] = held_status
        evidence["injected"] = True
        evidence["checks"].append("Actual foreground game and a real authenticated WSS press produce one acknowledged native Right Arrow key-down; game response remains unmeasured")

        receiver_identity = mapping.wait_for(lambda: native.receiver(args.receiver).get("identity"), "Receive-only focus target is not ready")
        first_event = len(native.injected_events(args.receiver))
        args.receiver.with_suffix(".focus").write_text("focus")
        paused = mapping.wait_for(lambda: (value if clean(value, "paused") else None) if (value := desktop(service)) else None,
            "Leaving the game foreground did not neutralize native ownership")
        mapping.wait_for(lambda: native.key_edge(native.injected_events(args.receiver)[first_event:], 77, False, 1), "Foreground receiver did not observe game focus-loss Right Arrow release")
        assert not native.native_press(native.injected_events(args.receiver)[first_event:]), "Out-of-scope receiver observed a new native press"
        assert windows.foreground()["identity"]["pid"] == receiver_identity["pid"]
        returned_focus = mapping.wait_for(lambda: windows.focus(identity, evidence["gameFocus"]["window"]), "Could not restore the exact game window", seconds=15)
        returned = mapping.wait_for(lambda: (value if value["runtime"]["state"] == "waiting" and "route-key" in value["runtime"]["waitingNeutral"] else None) if (value := desktop(service)) else None,
            "Returning to the game did not require a fresh neutral baseline")
        press_count = len(press_outcomes(returned))
        peer.input([dict(controlId="flight-input-0", value=True)])
        time.sleep(.2)
        repeated = desktop(service)
        assert clean(repeated, "waiting") and "route-key" in repeated["runtime"]["waitingNeutral"] and len(press_outcomes(repeated)) == press_count, repeated
        peer.input([dict(controlId="flight-input-0", value=False)])
        mapping.wait_for(lambda: "route-key" not in desktop(service)["runtime"]["waitingNeutral"], "Fresh neutral in returned game did not rearm")
        peer.input([dict(controlId="flight-input-0", value=True)])
        rearmed = mapping.wait_for(lambda: (value if value["runtime"]["backend"]["held"] == [RIGHT] and len(press_outcomes(value)) == press_count + 1 else None) if (value := desktop(service)) else None,
            "Fresh neutral then true did not produce a new game-scoped native hold")
        peer.input([dict(controlId="flight-input-0", value=False)])
        mapping.wait_for(lambda: clean(desktop(service), "active"), "Game-scoped hold did not release")
        evidence["focusReturn"] = dict(receiverIdentity=receiver_identity, paused=paused, returned=returned, repeatedTrue=repeated, rearmed=rearmed,
            returnedFocus=returned_focus, receiverObservations=native.injected_events(args.receiver)[first_event:])
        evidence["checks"].append("Focus away releases owned key; returning while held and repeated true stay disarmed until a new false then true")

        draft = copy.deepcopy(saved)
        draft["name"] += " saved draft"
        draft["desktopOutputs"][0]["actions"][0]["action"]["key"] = 80
        later = mapping.save(service, draft, assignments, expected=saved["revision"])
        unchanged = service.profiles("profile_status")["profileSession"]
        assert later["revision"] == saved["revision"] + 1 and unchanged["owner"] == run["owner"] and unchanged["appliedSnapshot"] == run["appliedSnapshot"], unchanged
        before_again = len(press_outcomes(desktop(service)))
        peer.input([dict(controlId="flight-input-0", value=True)])
        immutable = mapping.wait_for(lambda: (value if value["runtime"]["backend"]["held"] == [RIGHT] and len(press_outcomes(value)) == before_again + 1 else None) if (value := desktop(service)) else None,
            "Saving draft Left Arrow changed the applied Right Arrow snapshot")
        evidence.update(laterSavedProfile=later, immutableAppliedRun=unchanged, immutableAppliedHold=immutable)
        evidence["checks"].append("Saving draft Left Arrow increments revision while the applied owner, snapshot and native Right Arrow action remain unchanged")
        # Stop while physically held verifies cancellation, rather than merely
        # stopping an already neutral graph.
        stopped = service.profiles("profile_stop")["profileSession"]
        stopped_desktop = desktop(service)
        assert stopped["state"] == "stopped" and stopped["owner"] is None and clean(stopped_desktop, "stopped"), stopped_desktop
        assert stopped_desktop["runtime"]["backend"]["runs"] == []
        evidence["stopped"] = dict(profileSession=stopped, desktop=stopped_desktop)
        evidence["checks"].append("Native Stop while the companion button is held clears run ownership and all native held/possible-held state without uncertainty")
    except Exception as error:
        evidence["failure"] = str(error)
        try:
            evidence["desktopAtFailure"] = desktop(service)
            evidence["profileAtFailure"] = service.profiles("profile_status")["profileSession"]
        except Exception as observation:
            evidence["failureObservationError"] = str(observation)
        raise
    finally:
        cleanup_errors = []
        def cleanup(label, operation):
            try:
                return operation()
            except Exception as error:
                cleanup_errors.append(label + ": " + str(error))
        cleanup("Stop native profile", lambda: service.profiles("profile_stop"))
        cleanup("Restore desktop enablement/scope", lambda: mapping.state(service, "profile_desktop_configure", "desktop", enabled=before["enabled"], scope=before["scope"]))
        def restore_settings():
            if settings_existed:
                args.desktop_settings_file.write_bytes(settings_before)
            else:
                args.desktop_settings_file.unlink(missing_ok=True)
        cleanup("Restore exact desktop preference file", restore_settings)
        if peer:
            cleanup("Close test WSS peer", peer.close)
            cleanup("Revoke test identity", lambda: companion.operator(service, "companion_revoke", companionId=instance))
        if listener_enabled is False:
            cleanup("Restore disabled listener", lambda: companion.operator(service, "companion_disable"))
        # A failed launch can still have started the actual game. Never close a
        # pre-existing process, a recycled PID or any Steam process.
        if evidence["actualLaunchRequested"]:
            if identity is None:
                candidates = windows.matching(game["executable"])
                evidence["gameCleanup"] = dict(state="left-running-unverified" if candidates else "no-process-observed", processes=candidates,
                    reason="Launch did not return an owned process scope; no process was closed without that provenance.")
            if identity:
                evidence["gameCleanup"] = cleanup("Gracefully close exact task game", lambda: windows.close_task_game(identity))
        def verify():
            run, status = service.profiles("profile_status")["profileSession"], desktop(service)
            listener = companion.operator(service, "companion_status").get("listener", {})
            evidence["finalStatus"] = dict(profileSession=run, desktop=native.compact_desktop(status), listener=listener)
            assert run["state"] == "stopped" and run["owner"] is None and clean(status, "stopped"), run
            assert status["enabled"] == before["enabled"] and status["scope"] == before["scope"] and status["application"] == before["application"], status
            if listener_enabled is not None:
                assert listener.get("enabled", False) == listener_enabled
        cleanup("Verify stopped session and restored preferences", verify)
        evidence["cleanupErrors"] = cleanup_errors
        evidence["selectionAfterTest"] = "Stopped with Manual pinned to the test profile; no automatic reactivation was requested."
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
        if cleanup_errors and sys.exc_info()[0] is None:
            raise RuntimeError("; ".join(cleanup_errors))
    print(json.dumps(dict(checks=evidence["checks"], gameCleanup=evidence.get("gameCleanup"), observedGameBehavior=False), indent=2))


if __name__ == "__main__":
    main()
