#!/usr/bin/env python3
"""Real WSS peer -> running native Cockpit -> optional receive-only Windows window.

The service facade forwards native IPC to the already-running Cockpit. It never
starts a second gateway service, fabricates native ports, or submits OS input.
"""
import argparse
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
spec = importlib.util.spec_from_file_location("companion_native", ROOT / "scripts/companion-mapping-acceptance.py")
companion = importlib.util.module_from_spec(spec)
spec.loader.exec_module(companion)
mapping = companion.mapping
MACRO_CONTROL = "flight-input-6"
MACRO_KEY = {"kind": "key", "key": 5}  # ordinary KeyB, no modifiers
MACRO_STEPS = [dict(afterUs=0, pressed=True), dict(afterUs=1_500_000, pressed=False),
    dict(afterUs=6_000_000, pressed=True), dict(afterUs=7_000_000, pressed=False)]


class NativeCockpit:
    def __init__(self, endpoint):
        assert endpoint.startswith("http://127.0.0.1:") and endpoint.endswith("/native")
        self.endpoint = endpoint

    def request(self, command, **fields):
        request = urllib.request.Request(self.endpoint, json.dumps(dict(command=command, **fields)).encode(),
            {"Content-Type": "application/json", "Authorization": "Bearer " + os.environ["OSG_NATIVE_BRIDGE_TOKEN"]})
        with urllib.request.urlopen(request, timeout=30) as response:
            result = response.read(2 * 1024 * 1024 + 1)
        assert len(result) <= 2 * 1024 * 1024
        return json.loads(result)

    def profiles(self, command, **fields):
        return mapping.state(self, command, "profiles", **fields)


def receiver(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {"observations": []}


def injected_events(path):
    return [event for event in receiver(path)["observations"] if event.get("injectedMarker") is True]


def complete_key_edges(events, down):
    key_message = 256 if down else 257
    return all(any(event["message"] == key_message and event["scanCode"] == scan and event["extended"] == extended
        for event in events) for scan, extended in [(77, 1), (29, 0), (42, 0), (75, 1)])


def complete_edges(events, down):
    mouse_message = 513 if down else 514
    return complete_key_edges(events, down) and any(event["message"] == mouse_message for event in events)


def mouse_state_after(snapshot, identity, down, sequence, at_us, focus_requests=None):
    """Require new eligible-foreground OS state; never infer a client mouse event."""
    state = snapshot.get("mouseButtonState") or {}
    if (snapshot.get("identity") == identity and state.get("available") is True
        and state.get("swapButtons") is False and state.get("leftButtonDown") is down
        and int(state.get("sequence", "0")) > sequence and int(state.get("atUs", "0")) > at_us
        and (focus_requests is None or snapshot.get("focusRequests", 0) > focus_requests)):
        return state
    return None


def key_edge(events, scan, down, extended=0):
    return any(event["message"] == (256 if down else 257) and event["scanCode"] == scan
        and event["extended"] == extended for event in events)


def native_press(events):
    return any(event["message"] in [256, 260, 513, 516, 519, 523] for event in events)


def desktop_status(service):
    return mapping.state(service, "profile_desktop_status", "desktop")


def native_input_workers():
    """Read-only fail-closed worker absence check; never kill matching processes."""
    assert os.name == "nt", "Native ownership proof requires Windows process inspection"
    script = "$ErrorActionPreference='Stop'; $workers=@(Get-CimInstance Win32_Process -Filter \"Name = 'osg-cockpit.exe' OR Name = 'cockpit.exe'\" | Where-Object {$_.CommandLine -like '*--osg-desktop-input-worker*'} | ForEach-Object {[pscustomobject]@{pid=$_.ProcessId;name=$_.Name}}); ConvertTo-Json -InputObject $workers -Compress"
    result = subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script],
        capture_output=True, text=True, timeout=15, check=True, creationflags=subprocess.CREATE_NO_WINDOW)
    workers = json.loads(result.stdout)
    assert isinstance(workers, list), "Native worker inspection returned no bounded process list"
    return workers


def stopped_ownership(service, apply_attempted):
    """Require native session, ledger and process proof before destroying context."""
    profile, desktop = service.profiles("profile_status")["profileSession"], desktop_status(service)
    assert profile["state"] == "stopped" and profile["owner"] is None, profile
    runtime = desktop.get("runtime")
    if runtime is None:
        assert not apply_attempted, "No native ledger proves cleanup after attempted Apply"
    else:
        backend = runtime["backend"]
        assert runtime["state"] == "stopped", runtime
        assert backend["held"] == backend["possibleHeld"] == backend["owners"] == backend["runs"] == [], backend
        assert backend["cleanupComplete"] is True and backend["uncertain"] is False and backend["inFlight"] is None, backend
    workers = native_input_workers()
    assert workers == [], "A native input worker still owns the session"
    return dict(profileSession=profile, desktop=compact_desktop(desktop), nativeInputWorkers=workers)


def compact_desktop(status):
    runtime = status.get("runtime") or {}
    backend = runtime.get("backend") or {}
    return dict(enabled=status["enabled"], state=runtime.get("state"), scope=runtime.get("scope"),
        waitingNeutral=runtime.get("waitingNeutral", []), focusBoundaryUs=runtime.get("focusBoundaryUs"),
        backend={key: backend.get(key) for key in ["phase", "held", "possibleHeld", "uncertain", "cleanupComplete", "runs"]})


def focus_return(service, peer, first, second, evidence):
    """Receive-only windows request focus; only the applied native host injects."""
    second_identity = mapping.wait_for(lambda: receiver(second).get("identity"), "Second receive-only window is not ready")
    first_identity = receiver(first)["identity"]
    assert first_identity["pid"] != second_identity["pid"]
    assert first_identity["executable"] == second_identity["executable"]
    first_baseline = mapping.wait_for(lambda: receiver(first).get("mouseButtonState"), "Receiver has no bounded OS mouse-state observer")
    assert first_baseline["swapButtons"] is False, "OS mouse-state acceptance requires unswapped physical/logical buttons"
    first_start, second_start = len(injected_events(first)), len(injected_events(second))
    peer.input([dict(controlId="flight-input-0", value=True)])
    mapping.wait_for(lambda: complete_edges(injected_events(first)[first_start:], True), "Focus test hold did not reach scoped receiver")
    held_snapshot = mapping.wait_for(lambda: value if (value := receiver(first)).get("mouseButtonState") else None,
        "Scoped receiver did not publish a complete OS mouse-state snapshot")
    held_baseline = held_snapshot["mouseButtonState"]
    mouse_down_at = max(int(event["atUs"]) for event in held_snapshot["observations"]
        if event.get("injectedMarker") is True and event["message"] == 513)
    held_mouse_state = mapping.wait_for(lambda: mouse_state_after(receiver(first), first_identity, True,
        int(held_baseline["sequence"]), max(int(held_baseline["atUs"]), mouse_down_at)),
        "Scoped receiver did not freshly observe the OS left mouse button held")
    second_baseline = mapping.wait_for(lambda: value if (value := receiver(second)).get("mouseButtonState") else None,
        "Second receiver did not publish a complete OS mouse-state snapshot")
    second_mouse_baseline = second_baseline["mouseButtonState"]
    second.with_suffix(".focus").write_text("focus")

    def scope_paused():
        status = desktop_status(service)
        runtime, backend = status["runtime"], status["runtime"]["backend"]
        return status if runtime["state"] == "paused" and backend["held"] == [] and backend["possibleHeld"] == [] and backend["cleanupComplete"] and not backend["uncertain"] else None
    paused = mapping.wait_for(scope_paused, "Application focus loss did not pause output and clear native ownership")
    mapping.wait_for(lambda: complete_key_edges(injected_events(second)[second_start:], False), "Second foreground receiver did not observe all four focus-loss key releases")
    released_mouse_state = mapping.wait_for(lambda: mouse_state_after(receiver(second), second_identity, False,
        int(second_mouse_baseline["sequence"]), int(second_mouse_baseline["atUs"]), second_baseline["focusRequests"]),
        "Second eligible foreground receiver did not freshly observe the OS left mouse button released")
    second_cleanup_events = injected_events(second)[second_start:]
    second_observed_mouse_up = any(event["message"] == 514 for event in second_cleanup_events)
    assert not native_press(second_cleanup_events), "Out-of-scope receiver observed a new native press"
    first_return = len(injected_events(first))
    first.with_suffix(".focus").write_text("focus")
    routes = {"route-key", "route-chord", "route-mouse"}

    def waiting_after_return():
        status = desktop_status(service)
        runtime = status["runtime"]
        return status if runtime["state"] == "waiting" and runtime["focusBoundaryUs"] is not None and routes.issubset(runtime["waitingNeutral"]) else None
    returned = mapping.wait_for(waiting_after_return, "Returning application scope did not require fresh neutral evidence")
    time.sleep(.15)
    assert not native_press(injected_events(first)[first_return:]), "Focus return replayed the held input"
    peer.input([dict(controlId="flight-input-0", value=True)])
    time.sleep(.15)
    repeated = desktop_status(service)
    assert routes.issubset(repeated["runtime"]["waitingNeutral"]), "Repeated true input rearmed without neutral"
    assert repeated["runtime"]["backend"]["held"] == []
    assert not native_press(injected_events(first)[first_return:]), "Repeated true input produced a native press without neutral"
    peer.input([dict(controlId="flight-input-0", value=False)])

    def fresh_neutral():
        status = desktop_status(service)
        return status if routes.isdisjoint(status["runtime"]["waitingNeutral"]) else None
    neutral = mapping.wait_for(fresh_neutral, "Fresh neutral input did not establish a rearm baseline")
    rearmed_start = len(injected_events(first))
    peer.input([dict(controlId="flight-input-0", value=True)])
    mapping.wait_for(lambda: complete_edges(injected_events(first)[rearmed_start:], True), "Fresh neutral then true did not produce a new native press")
    peer.input([dict(controlId="flight-input-0", value=False)])
    mapping.wait_for(lambda: complete_edges(injected_events(first)[rearmed_start:], False), "Rearmed input did not release all native holds")
    assert not native_press(injected_events(second)[second_start:]), "Second receiver observed an out-of-scope native press"
    evidence["focusReturn"] = dict(firstIdentity=first_identity, secondIdentity=second_identity,
        beforeFirstEvent=first_start, beforeSecondEvent=second_start, firstReturnEvent=first_return, rearmedFirstEvent=rearmed_start,
        heldMouseButtonState=held_mouse_state, releasedMouseButtonState=released_mouse_state,
        secondReceiverObservedMouseUp=second_observed_mouse_up,
        mouseCleanupBoundary="Fresh eligible-foreground OS VK_LBUTTON high-bit state changed from down to up with unswapped buttons; no focus-loss client mouse-up recipient claim",
        paused=compact_desktop(paused), returnedHeld=compact_desktop(returned), repeatedTrue=compact_desktop(repeated), freshNeutral=compact_desktop(neutral),
        firstObservations=injected_events(first)[first_start:], secondObservations=injected_events(second)[second_start:])
    evidence["checks"].append("Actual application focus loss clears native ownership and OS left-button state; the second eligible foreground receiver observes all four keyboard releases, without claiming a mouse-up recipient; focus return and repeated true stay disarmed until fresh false then true")


def macro_focus_return(service, peer, owner, first, second, evidence):
    """Cancel an upstream Macro without letting its old timer resume desktop output.

    All deadlines use the applied native Phase clock. Repeated real WSS state
    reports keep the source fresh while the independent native macro advances.
    """
    first_start, second_start = len(injected_events(first)), len(injected_events(second))

    def edges(path, start):
        return [event for event in injected_events(path)[start:] if event["message"] in (256, 257)
            and event["scanCode"] == 48 and event["extended"] == 0]

    def inspection():
        page = mapping.inspect(service, owner, "focus-macro")
        ports = {port["endpoint"]["portId"]: port for port in page["ports"]}
        return dict(capturedUs=page["capturedUs"], input=ports["in"], output=ports["out"])

    def known_false(port):
        return port["sample"] == dict(status="known", value=dict(kind="scalar", value=False))

    def known_true(port):
        return port["sample"] == dict(status="known", value=dict(kind="scalar", value=True))

    def hold_outcomes(status):
        return [outcome for outcome in status["runtime"].get("outcomes", [])
            if outcome["delivery"]["intent"] == dict(kind="held", controls=[MACRO_KEY])
            and outcome["acknowledgement"]["attempted"] > 0]

    def send(value):
        peer.input([dict(controlId=MACRO_CONTROL, value=value)])

    def no_native_hold(status, state):
        runtime, backend = status["runtime"], status["runtime"]["backend"]
        eligible = runtime["state"] in ("active", "waiting") if state == "focused" else runtime["state"] == state
        return (eligible and backend["held"] == [] and backend["possibleHeld"] == []
            and backend["cleanupComplete"] and not backend["uncertain"])

    def cancelled_edges():
        first_edges, second_edges = edges(first, first_start), edges(second, second_start)
        assert [event["message"] for event in first_edges] == [256], "An old macro timer resumed a native KeyB edge in the scoped receiver"
        assert [event["message"] for event in second_edges] == [257], "An old macro timer pressed KeyB outside application scope"

    def advance_to(target_us, source_value, check, seconds=20):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            send(source_value)
            sample, status = inspection(), desktop_status(service)
            check(sample, status)
            if int(sample["capturedUs"]) >= target_us:
                return sample, status
            time.sleep(.15)
        raise AssertionError("The applied native macro clock did not reach its bounded test deadline")

    send(False)
    mapping.wait_for(lambda: "route-macro" not in desktop_status(service)["runtime"]["waitingNeutral"], "Fresh macro neutral did not arm its desktop route")
    send(True)
    def initial_hold():
        status = desktop_status(service)
        outcomes = hold_outcomes(status)
        return (status, outcomes[-1]) if outcomes and status["runtime"]["backend"]["held"] == [MACRO_KEY] else None
    held, first_outcome = mapping.wait_for(initial_hold, "Real WSS macro trigger did not hold KeyB")
    ack = first_outcome["acknowledgement"]
    assert ack["ok"] and ack["attempted"] == ack["confirmed"] == 1 and not ack["uncertain"] and not ack["skipped"], ack
    mapping.wait_for(lambda: [event["message"] for event in edges(first, first_start)] == [256], "Scoped receiver did not observe the first macro KeyB press")
    start_us = int(first_outcome["delivery"]["atUs"])
    second.with_suffix(".focus").write_text("focus")
    paused = mapping.wait_for(lambda: (status if no_native_hold(status, "paused") else None)
        if (status := desktop_status(service)) else None, "Macro focus loss did not pause and release native ownership")
    mapping.wait_for(lambda: [event["message"] for event in edges(second, second_start)] == [257], "Foreground receiver did not observe macro focus-loss KeyB release")
    cancelled_edges()

    def check_paused(_sample, status):
        assert no_native_hold(status, "paused"), "Native macro output became active outside scope"
        cancelled_edges()
    intermediate, _ = advance_to(start_us + MACRO_STEPS[1]["afterUs"] + 250_000, True, check_paused)
    assert known_false(intermediate["output"]), "The original native macro did not reach its intermediate release while out of scope"
    assert int(intermediate["capturedUs"]) < start_us + MACRO_STEPS[2]["afterUs"] - 1_000_000, "Focus-return setup exceeded the macro's generous pending-step window"
    first.with_suffix(".focus").write_text("focus")
    returned = mapping.wait_for(lambda: (status if status["runtime"]["state"] == "waiting" and "route-macro" in status["runtime"]["waitingNeutral"] else None)
        if (status := desktop_status(service)) else None, "Macro focus return did not require fresh source neutral")
    send(False)
    def fresh_neutral():
        status, sample = desktop_status(service), inspection()
        return (sample, status) if known_false(sample["input"]) and "route-macro" not in status["runtime"]["waitingNeutral"] and no_native_hold(status, "focused") else None
    neutral, neutral_status = mapping.wait_for(fresh_neutral, "Fresh macro root neutral did not rearm without resuming its pending timer")
    cancelled_edges()
    time.sleep(.05)
    send(False)
    def unchanged_neutral():
        status, sample = desktop_status(service), inspection()
        return (sample, status) if known_false(sample["input"]) and int(sample["input"]["observedUs"] or 0) > int(neutral["input"]["observedUs"] or 0) else None
    repeated, repeated_status = mapping.wait_for(unchanged_neutral, "A newer unchanged native false report was not observed")
    assert int(repeated["capturedUs"]) < start_us + MACRO_STEPS[2]["afterUs"] - 500_000, "Neutral reports arrived too late to test the old pending press"
    saw_original_later_press = False
    def check_cancelled(sample, status):
        nonlocal saw_original_later_press
        assert no_native_hold(status, "focused") and "route-macro" not in status["runtime"]["waitingNeutral"], "Fresh neutral allowed an old macro timer to regain native ownership"
        cancelled_edges()
        saw_original_later_press |= known_true(sample["output"])
    past_old_deadline, cancelled_status = advance_to(start_us + MACRO_STEPS[-1]["afterUs"] + 500_000, False, check_cancelled)
    assert saw_original_later_press, "The independent applied macro never reached its old later press; cancellation was not exercised"
    assert known_false(past_old_deadline["output"])

    # A fresh rising source edge must still create a complete new macro after
    # cancellation. Keep true fresh without creating another rising edge.
    rearmed_start = len(injected_events(first))
    original_holds = len(hold_outcomes(cancelled_status))
    send(True)
    def new_hold():
        status = desktop_status(service)
        outcomes = hold_outcomes(status)
        return (status, outcomes[-1]) if len(outcomes) == original_holds + 1 and status["runtime"]["backend"]["held"] == [MACRO_KEY] else None
    rearmed, new_outcome = mapping.wait_for(new_hold, "Fresh neutral then true did not start a new native desktop macro")
    new_start_us = int(new_outcome["delivery"]["atUs"])
    def check_new_macro(_sample, status):
        assert status["runtime"]["state"] in ("active", "waiting") and "route-macro" not in status["runtime"]["waitingNeutral"], "Fresh macro lost verified application focus or neutral arming"
        assert not native_press(injected_events(second)[second_start:]), "Fresh macro pressed outside application scope"
        messages = [event["message"] for event in edges(first, rearmed_start)]
        assert messages == [256, 257, 256, 257][:len(messages)], "Fresh macro produced duplicate or reordered KeyB edges"
    completed, completed_status = advance_to(new_start_us + MACRO_STEPS[-1]["afterUs"] + 500_000, True, check_new_macro)
    send(False)
    mapping.wait_for(lambda: [event["message"] for event in edges(first, rearmed_start)] == [256, 257, 256, 257], "Fresh desktop macro did not complete its two KeyB press/release pairs")
    assert no_native_hold(desktop_status(service), "focused")
    evidence["macroFocusReturn"] = dict(controlId=MACRO_CONTROL, key=MACRO_KEY, steps=MACRO_STEPS,
        initialNativeOutcome=first_outcome, paused=compact_desktop(paused), intermediate=intermediate,
        returned=compact_desktop(returned), freshNeutral=neutral, freshNeutralStatus=compact_desktop(neutral_status),
        unchangedNeutral=repeated, unchangedNeutralStatus=compact_desktop(repeated_status),
        oldDeadlinePassed=past_old_deadline, originalGlobalMacroLaterPressObserved=saw_original_later_press,
        cancelledStatus=compact_desktop(cancelled_status), newNativeOutcome=new_outcome,
        newMacroComplete=completed, newMacroStatus=compact_desktop(completed_status),
        firstObservations=injected_events(first)[first_start:], secondObservations=injected_events(second)[second_start:])
    evidence["checks"].append("Application focus loss cancels an upstream native Macro: its old later press never reaches desktop after fresh neutral and another unchanged false; a fresh press starts a new complete KeyB macro")


def encoder_burst(peer, count):
    """One socket write retains every ordered WSS input frame and its ACK."""
    with peer.lock:
        frames = []
        for _ in range(count):
            for value in [True, False]:
                peer.sequence += 1
                frames.append(companion.encoded_frame(peer.message("input", sequence=str(peer.sequence),
                    changes=[dict(controlId="flight-input-5", value=value)])))
        peer.socket.socket.sendall(b"".join(frames))
        for _ in frames:
            reply = peer.socket.frame()
            assert reply["ok"], reply


def make_profile(service, instance, publication):
    detail = mapping.wait_for(lambda: companion.selected_resource(service, instance), "No exact current companion ports in native inventory")
    observed = {logical: next(port for port in detail["ports"] if port["descriptor"]["id"] == native)
        for logical, native in [("button", "flight-input-0"), ("shared", "flight-input-1"), ("encoder", "flight-input-5"), ("axis", "flight-input-2"), ("macro", MACRO_CONTROL)]}
    assert all(port["current"] and port["descriptor"]["direction"] == "output" for port in observed.values())
    contract = observed["button"]["descriptor"]["contract"]
    assert contract["delivery"] == "state" and contract["schema"] == {"type": "boolean"}
    document = mapping.blank("Native companion desktop " + str(uuid.uuid4())[:8])
    document["resources"] = [dict(id="panel", name="Real authenticated WSS panel", kind="companion", required=False,
        ports=[mapping.port(logical, port["descriptor"]["contract"], "output") for logical, port in observed.items()])]
    state = dict(delivery="state", schema=dict(type="boolean"), freshnessMs=1000, fallback=False)
    actions = [dict(portId="key", action=dict(kind="key-hold", key=79)),
        dict(portId="chord", action=dict(kind="chord", keys=[224, 225, 80])),
        dict(portId="mouse", action=dict(kind="mouse-hold", button="left")),
        dict(portId="shared", action=dict(kind="chord", keys=[224, 10])),
        dict(portId="tap", action=dict(kind="key-tap", key=40, downMs=30, upMs=30)),
        dict(portId="pointer", action=dict(kind="pointer", axis="x", speed=100, curve=1)),
        dict(portId="macro", action=dict(kind="key-hold", key=5))]
    event = dict(delivery="event", schema=dict(type="boolean"))
    normalized = dict(delivery="state", schema=dict(type="number", min=-1, max=1, unit=None), freshnessMs=1000, fallback=0)
    document["desktopOutputs"] = [dict(id="desktop", name="Scoped receiver keyboard and mouse", required=True,
        ports=[mapping.port(action["portId"], event if action["portId"] == "tap" else normalized if action["portId"] == "pointer" else state, "input") for action in actions], actions=actions)]
    document["bindings"] = [dict(id="route-" + action["portId"], **{"from": dict(nodeId="panel", portId="button"),
        "to": dict(nodeId="desktop", portId=action["portId"])}) for action in actions[:3]]
    document["processingNodes"] = [dict(id="encoder-edge", name="Visible encoder press edge", operation=dict(kind="event-edge", edge="rising"),
        inputs=[mapping.port("in", observed["encoder"]["descriptor"]["contract"], "input")], outputs=[mapping.port("out", event, "output")]),
        dict(id="pointer-normalize", name="Visible raw axis normalization", operation=dict(kind="scale", factor=1 / 32768, offset=0),
        inputs=[mapping.port("in", observed["axis"]["descriptor"]["contract"], "input")], outputs=[mapping.port("out", normalized, "output")]),
        dict(id="focus-macro", name="Visible macro cancelled at desktop focus boundary",
            operation=dict(kind="builtin", inputs=["in"], rule=dict(kind="macro", steps=MACRO_STEPS)),
            inputs=[mapping.port("in", observed["macro"]["descriptor"]["contract"], "input")], outputs=[mapping.port("out", state, "output")])]
    for identity, source_node, source_port, target_node, target_port in [
        ("shared", "panel", "shared", "desktop", "shared"), ("encoder-input", "panel", "encoder", "encoder-edge", "in"),
        ("encoder-tap", "encoder-edge", "out", "desktop", "tap"), ("pointer-input", "panel", "axis", "pointer-normalize", "in"),
        ("pointer-output", "pointer-normalize", "out", "desktop", "pointer"),
        ("macro-input", "panel", "macro", "focus-macro", "in"), ("route-macro", "focus-macro", "out", "desktop", "macro")]:
        document["bindings"].append(dict(id=identity, **{"from": dict(nodeId=source_node, portId=source_port), "to": dict(nodeId=target_node, portId=target_port)}))
    document["companionAssignments"] = [dict(resourceId="panel", publicationHash=publication["publicationHash"])]
    assignments = [dict(resourceId="panel", sourceId=detail["resource"]["id"], ports=[dict(profilePortId=logical,
        observedPortId=port["descriptor"]["id"], expectedContractHash=port["contractHash"]) for logical, port in observed.items()])]
    return document, assignments, detail


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bridge", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--inject", action="store_true")
    parser.add_argument("--receiver", type=Path)
    parser.add_argument("--focus-receiver", type=Path)
    parser.add_argument("--desktop-settings-file", type=Path)
    args = parser.parse_args()
    assert not args.inject or os.environ.get("OSG_DESKTOP_NATIVE_INJECT") == "1", "Native injection needs explicit test opt-in"
    service = NativeCockpit(args.bridge)
    publication = json.loads((ROOT / "crates/companion-service/tests/fixtures/c1-flight-publication.json").read_text())
    instance = "desktop-acceptance-" + uuid.uuid4().hex[:12]
    before = service.profiles("profile_status")["profileSession"]
    desktop_before = mapping.state(service, "profile_desktop_status", "desktop")
    if args.inject:
        assert before["state"] == "stopped" and before["owner"] is None, "Another native profile owns the session"
        assert native_input_workers() == [], "Another native input worker is active"
    settings_existed = args.desktop_settings_file is not None and args.desktop_settings_file.exists()
    settings_before = args.desktop_settings_file.read_bytes() if settings_existed else None
    evidence = dict(actualNativeCockpit=True, source="Authenticated pinned-CA loopback WSS peer", injected=False,
        virtualControllers=0, productionQualified=False, checks=[], inputsSent=[],
        timing="Python monotonic input-send timestamps and receiver-local microseconds are separate uncorrelated clocks; no latency claim.")
    peer = None
    apply_attempted = False
    listener_enabled = False
    try:
        status = companion.operator(service, "companion_status")
        listener_enabled = status.get("listener", {}).get("enabled", False)
        if not listener_enabled:
            with socket.socket() as probe:
                probe.bind(("127.0.0.1", 0))
                port = probe.getsockname()[1]
            companion.operator(service, "companion_enable", address="127.0.0.1", advertisedHost="127.0.0.1", port=port, discovery=False)
        companion.publish(service, publication)
        peer = companion.Peer(service, instance)
        companion.operator(service, "companion_assign", companionId=instance, instanceId=instance, publicationHash=publication["publicationHash"])
        peer.open(instance, publication)
        document, assignments, detail = make_profile(service, instance, publication)
        saved = mapping.save(service, document, assignments)
        compiled = mapping.state(service, "profile_compile", "profileCompilation", document=saved, assignments=assignments)
        assert compiled["readiness"] != "invalid", compiled
        assert compiled["requirements"] == dict(desktopInput=True, virtualDevices=False, visibility=False), compiled
        assert compiled["budgets"]["virtualTargets"] == 0
        evidence.update(profile=saved, assignments=assignments, compilation=compiled, sourceResource=detail["resource"])
        evidence["checks"].append("Actual WSS pairing, publication, neutral baseline and exact native contract/hash pins compile to desktopInput with zero virtual controllers")
        if args.inject:
            assert args.receiver and args.focus_receiver and desktop_before["qualified"], "Only a qualification build may explicitly apply the desktop branch"
            identity = mapping.wait_for(lambda: receiver(args.receiver).get("identity"), "Receive-only window is not ready")
            args.receiver.with_suffix(".focus").write_text("focus")
            mapping.wait_for(lambda: desktop_status(service).get("recentApplication", {}).get("pid") == identity["pid"], "Native foreground observer did not see the first receiver")
            selected = mapping.state(service, "profile_desktop_capture_application", "desktop")
            assert selected["application"]["pid"] == identity["pid"], selected
            mapping.state(service, "profile_desktop_configure", "desktop", enabled=True, scope=dict(kind="application"))
            service.profiles("profile_pause", paused=False)
            apply_attempted = True
            owner = mapping.apply(service, saved)
            time.sleep(.2)
            initial_count = len(injected_events(args.receiver))
            evidence["inputsSent"].append(dict(controlId="flight-input-0", value=True, monotonicNs=str(time.perf_counter_ns())))
            peer.input([dict(controlId="flight-input-0", value=True)])
            mapping.wait_for(lambda: complete_edges(injected_events(args.receiver)[initial_count:], True), "Receiver saw no complete injected keyboard/chord/mouse holds")
            evidence["inputsSent"].append(dict(controlId="flight-input-0", value=False, monotonicNs=str(time.perf_counter_ns())))
            peer.input([dict(controlId="flight-input-0", value=False)])
            mapping.wait_for(lambda: complete_edges(injected_events(args.receiver)[initial_count:], False), "Receiver saw no complete injected keyboard/chord/mouse releases")
            evidence["checks"].append("Real companion press/release produced receiver-observed key, chord and mouse edges through the applied ProfileSession")

            shared_start = len(injected_events(args.receiver))
            peer.input([dict(controlId="flight-input-0", value=True)])
            mapping.wait_for(lambda: complete_edges(injected_events(args.receiver)[shared_start:], True), "First shared-modifier owner did not press")
            peer.input([dict(controlId="flight-input-1", value=True)])
            mapping.wait_for(lambda: key_edge(injected_events(args.receiver)[shared_start:], 34, True), "Second shared-modifier owner did not press G")
            first_release = len(injected_events(args.receiver))
            peer.input([dict(controlId="flight-input-0", value=False)])
            mapping.wait_for(lambda: key_edge(injected_events(args.receiver)[first_release:], 77, False, 1), "First shared owner did not release its own key")
            assert not key_edge(injected_events(args.receiver)[first_release:], 29, False), "Releasing one route released another route's Ctrl"
            evidence["sharedOwnerStatus"] = mapping.state(service, "profile_desktop_status", "desktop")
            peer.input([dict(controlId="flight-input-1", value=False)])
            mapping.wait_for(lambda: key_edge(injected_events(args.receiver)[first_release:], 29, False), "Final shared owner did not release Ctrl")
            evidence["checks"].append("Two actual companion controls share native Left Ctrl; releasing the first route preserves Ctrl until its final owner releases")

            tap_start = len(injected_events(args.receiver))
            burst_start = time.perf_counter_ns()
            encoder_burst(peer, 10)
            evidence["inputsSent"].append(dict(event="ten-detent-single-WSS-write", frames=20, startMonotonicNs=str(burst_start), endMonotonicNs=str(time.perf_counter_ns())))
            def ten_taps():
                events = injected_events(args.receiver)[tap_start:]
                downs = sum(event["message"] == 256 and event["scanCode"] == 28 and event["extended"] == 0 for event in events)
                ups = sum(event["message"] == 257 and event["scanCode"] == 28 and event["extended"] == 0 for event in events)
                assert downs <= 10 and ups <= 10, "Encoder burst produced duplicate native taps"
                return downs == ups == 10
            mapping.wait_for(ten_taps, "Ten ordered WSS encoder detents did not produce ten serialized receiver key taps", seconds=5)
            evidence["checks"].append("Twenty ordered encoder edges in one real WSS socket write produce ten serialized Enter taps with configured 30ms-down/30ms-up dwell; no measured dwell or native polling interval claim")

            pointer_start = len(injected_events(args.receiver))
            evidence["inputsSent"].append(dict(controlId="flight-input-2", value=.75, monotonicNs=str(time.perf_counter_ns())))
            peer.input([dict(controlId="flight-input-2", value=.75)])
            mapping.wait_for(lambda: sum(event["message"] == 512 for event in injected_events(args.receiver)[pointer_start:]) >= 2, "Deflected source did not move the native receiver pointer")
            first_motion = len(injected_events(args.receiver))
            time.sleep(.15)
            assert any(event["message"] == 512 for event in injected_events(args.receiver)[first_motion:]), "Stationary deflection stopped without another input report"
            peer.input([dict(controlId="flight-input-2", value=0), dict(controlId="flight-input-0", value=False)])
            time.sleep(.1)
            evidence["checks"].append("One actual companion axis update sustains receiver-observed relative motion across explicit host ticks without further axis reports")

            focus_return(service, peer, args.receiver, args.focus_receiver, evidence)
            peer.input([dict(controlId="flight-input-0", value=False), dict(controlId="flight-input-1", value=False),
                dict(controlId="flight-input-5", value=False), dict(controlId=MACRO_CONTROL, value=False), dict(controlId="flight-input-2", value=0)])
            mapping.wait_for(lambda: not desktop_status(service)["runtime"]["waitingNeutral"], "Fresh neutral controls did not rearm every route after the focus check")
            macro_focus_return(service, peer, owner, args.receiver, args.focus_receiver, evidence)
            peer.input([dict(controlId="flight-input-0", value=False), dict(controlId="flight-input-1", value=False),
                dict(controlId="flight-input-5", value=False), dict(controlId=MACRO_CONTROL, value=False), dict(controlId="flight-input-2", value=0)])
            mapping.wait_for(lambda: not desktop_status(service)["runtime"]["waitingNeutral"], "Fresh neutral controls did not rearm every route after the macro focus check")

            # Mouse messages go to the cursor location, independently of the
            # keyboard foreground scope. Re-center this owned receive-only
            # client before another mouse hold, avoiding title-bar move loops.
            focus_requests = receiver(args.receiver).get("focusRequests", 0)
            args.receiver.with_suffix(".focus").write_text("focus-and-center")
            positioned = mapping.wait_for(lambda: (value if value.get("focusRequests", 0) > focus_requests
                and value.get("clientCursor", {}).get("inside") is True else None)
                if (value := receiver(args.receiver)) else None, "Receive-only client cursor was not positioned for the lease check")
            evidence["receiverClientBeforeLease"] = positioned["clientCursor"]
            loss_count = len(injected_events(args.receiver))
            evidence["inputsSent"].append(dict(controlId="flight-input-0", value=True, monotonicNs=str(time.perf_counter_ns())))
            peer.input([dict(controlId="flight-input-0", value=True), dict(controlId="flight-input-2", value=.5)])
            mapping.wait_for(lambda: complete_edges(injected_events(args.receiver)[loss_count:], True), "Second press never reached the receiver before the loss check")
            peer.paused.set()
            evidence["inputsSent"].append(dict(event="heartbeat-paused", monotonicNs=str(time.perf_counter_ns())))
            mapping.wait_for(lambda: complete_edges(injected_events(args.receiver)[loss_count:], False), "Heartbeat loss retained injected held keys or mouse buttons", seconds=8)
            time.sleep(.1)
            loss_motion = sum(event["message"] == 512 for event in injected_events(args.receiver))
            time.sleep(.15)
            assert sum(event["message"] == 512 for event in injected_events(args.receiver)) == loss_motion, "Pointer motion continued after source lease loss"
            evidence["runtimeAfterLeaseLoss"] = mapping.state(service, "profile_desktop_status", "desktop")
            service.profiles("profile_stop")
            evidence["receiver"] = receiver(args.receiver)
            evidence["focusReceiver"] = receiver(args.focus_receiver)
            evidence["appliedOwner"] = owner
            evidence["injected"] = True
            evidence["checks"].append("Companion heartbeat loss and native Stop cancel owned contributions without a virtual target")
        else:
            assert service.profiles("profile_status")["profileSession"] == before
            after = mapping.state(service, "profile_desktop_status", "desktop")
            assert after["enabled"] == desktop_before["enabled"] and after["scope"] == desktop_before["scope"]
            evidence["checks"].append("Prepare-only capture/save/preflight leave native ProfileSession and desktop enablement/scope unchanged")
    except Exception as error:
        evidence["failure"] = str(error)
        if args.receiver:
            evidence["receiver"] = receiver(args.receiver)
        if args.focus_receiver:
            evidence["focusReceiver"] = receiver(args.focus_receiver)
        try:
            evidence["runtimeAtFailure"] = mapping.state(service, "profile_desktop_status", "desktop")
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
        ownership_clear = not args.inject
        if args.inject:
            stop_confirmed = cleanup("Stop native profile", lambda: service.profiles("profile_stop")) is not None
        if peer:
            cleanup("Close WSS peer", peer.close)
        if args.inject:
            def prove_cleanup():
                assert stop_confirmed, "Native Stop did not succeed; preserve recovery settings and receivers"
                evidence["ownershipCleanupProof"] = stopped_ownership(service, apply_attempted)
                return True
            ownership_clear = cleanup("Prove stopped native ownership before restoring preferences", prove_cleanup) is True
            evidence["ownershipCleanupConfirmed"] = ownership_clear
            evidence["needsRootCleanup"] = not ownership_clear
        if args.inject and ownership_clear:
            cleanup("Restore local desktop enablement/scope", lambda: mapping.state(service, "profile_desktop_configure", "desktop", enabled=desktop_before["enabled"], scope=desktop_before["scope"]))
            if args.desktop_settings_file:
                def restore_settings():
                    if settings_existed:
                        args.desktop_settings_file.write_bytes(settings_before)
                    else:
                        args.desktop_settings_file.unlink(missing_ok=True)
                cleanup("Restore exact local desktop preferences", restore_settings)
        if peer and ownership_clear:
            cleanup("Revoke test WSS identity", lambda: companion.operator(service, "companion_revoke", companionId=instance))
        if ownership_clear and not listener_enabled:
            cleanup("Restore disabled companion listener", lambda: companion.operator(service, "companion_disable"))
        def verify_cleanup():
            profile = service.profiles("profile_status")["profileSession"]
            desktop = desktop_status(service)
            listener = companion.operator(service, "companion_status").get("listener", {})
            evidence["finalStatus"] = dict(profileSession=profile, desktop=compact_desktop(desktop), listener=listener)
            assert profile["state"] == "stopped" and profile["owner"] is None, profile
            assert desktop["enabled"] == desktop_before["enabled"] and desktop["scope"] == desktop_before["scope"], desktop
            if args.desktop_settings_file:
                assert desktop["application"] == desktop_before["application"], "Desktop application preference was not restored"
                assert args.desktop_settings_file.exists() == settings_existed
                assert not settings_existed or args.desktop_settings_file.read_bytes() == settings_before, "Desktop preference bytes were not restored"
                evidence["exactDesktopPreferencesRestored"] = True
            evidence["finalOwnershipProof"] = stopped_ownership(service, apply_attempted)
            assert listener.get("enabled", False) == listener_enabled, listener
        if args.inject and ownership_clear:
            cleanup("Verify stopped native session and restored preferences/listener", verify_cleanup)
        evidence["cleanupErrors"] = cleanup_errors
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
        if cleanup_errors and sys.exc_info()[0] is None:
            raise RuntimeError("; ".join(cleanup_errors))
    print(json.dumps({"profileId": evidence["profile"]["id"], "checks": evidence["checks"], "injected": evidence["injected"]}, indent=2))


if __name__ == "__main__":
    main()
