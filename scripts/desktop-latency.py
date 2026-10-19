#!/usr/bin/env python3
"""Offline native WSS admission -> SendInput-return audit (never physical capture).

Only immediate direct KeyHold changes are eligible. Numeric aliases are local to
the exact parent PID/QPC instance and run nonce. Logical capturedUs/observation
times are identity fields only; elapsed time uses QPC exclusively.
"""
import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass
import json
from pathlib import Path
import re
import sys

U64 = (1 << 64) - 1
MAX_FILE = 32 * 1024 * 1024
MAX_TOTAL = 64 * 1024 * 1024
SCHEMA = dict(SourceCapture=8, SourceResult=3, SourceBound=8, NativeEnqueue=12,
    NativeDequeue=8, TypedAlias=15, SyntheticFrame=9, Effect=6, EffectRoot=7,
    DeliveryBound=6, DeliveryDropped=5, ChannelOpened=2, ChannelExited=5, RequestIssued=6,
    RequestQueued=3, RequestDequeued=3, SubmitReturned=6, WorkerOutcome=6,
    ParentOutcome=7, QueueFault=3, NativeGap=4, Marker=2, Fault=1,
    JournalPersist=9)
ERROR_COUNTERS = ("queueDrops", "capDrops", "aliasFailures", "qpcFailures", "invalidFields",
    "serializationErrors", "ioErrors", "flushTimeouts", "evidenceFaults", "afterFinish")
COUNTERS = ("attempted", "enqueued", "written", *ERROR_COUNTERS, "queueHighWater",
    "lastEnqueuedSequence", "lastWrittenSequence")
EXCLUDED = ("synthetic", "noop", "collision", "skipped", "cleanup", "unacked", "fault", "unresolved")
OK, UNCERTAIN, CLEAN, SKIPPED, COLLISION = 1, 2, 4, 8, 16


class AuditError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise AuditError(message)


def uint(value, label="integer", positive=False):
    require(isinstance(value, str) and re.fullmatch(r"0|[1-9][0-9]*", value) is not None,
        f"{label} is not a canonical decimal string")
    number = int(value)
    require(number <= U64 and (number > 0 or not positive), f"{label} is outside its bounds")
    return number


def unique_json(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON object key")
        result[key] = value
    return result


@dataclass(frozen=True)
class Event:
    kind: str
    f: tuple
    qpc: int
    sequence: int


@dataclass
class Trace:
    header: dict
    events: list
    report: dict
    frequency: int
    identity: tuple
    channel: int | None
    ignored_tail: int = 0


@dataclass(frozen=True)
class Phase:
    name: str
    begin: int
    end: int
    expected_edges: int
    marker_id: int | None = None
    begin_id: int | None = None
    end_id: int | None = None


def validate_report(row, seen, last, finished):
    report = row.get("report")
    require(isinstance(report, dict), "missing collector report")
    require(all(report.get(key) is True for key in ("configured", "enabled", "complete", "valid")), "collector report is not valid/complete/enabled")
    require(report.get("finished") is finished, "collector finish state mismatch")
    counters = report.get("counters")
    require(isinstance(counters, dict) and set(counters) == set(COUNTERS), "collector counter schema mismatch")
    numbers = {key: uint(value, key) for key, value in counters.items()}
    require(not any(numbers[key] for key in ERROR_COUNTERS), "nonzero collector loss/fault counters")
    through = uint(report.get("throughSequence"), "checkpoint sequence")
    require(through == last == numbers["lastWrittenSequence"] and numbers["written"] == seen,
        "collector written prefix disagrees with JSONL records")
    require(numbers["attempted"] == numbers["enqueued"] == numbers["lastEnqueuedSequence"]
        and numbers["enqueued"] >= seen, "collector admission counters disagree")
    require(numbers["queueHighWater"] <= 4096, "collector queue bound exceeded")
    if finished:
        require(numbers["enqueued"] == seen, "worker footer has undrained events")
    return report


def parse_trace(path):
    with path.open("rb") as stream:
        data = stream.read(MAX_FILE + 1)
    require(len(data) <= MAX_FILE and data.endswith(b"\n"), "oversized or incomplete audit file")
    rows = []
    for raw in data.splitlines():
        require(0 < len(raw) <= 4096, "empty or oversized JSONL row")
        try:
            row = json.loads(raw, object_pairs_hook=unique_json)
        except (json.JSONDecodeError, UnicodeDecodeError) as error:
            raise AuditError("malformed audit JSON") from error
        require(isinstance(row, dict), "audit row is not an object")
        if row.get("type") == "event":
            require(len(raw) <= 512, "event exceeds collector numeric-record bound")
        rows.append(row)
    require(rows and rows[0].get("type") == "header", "missing audit header")
    header = rows[0]
    require(header.get("version") == "1" and header.get("role") in ("parent", "worker"), "unsupported audit version/role")
    require(isinstance(header.get("run"), str) and re.fullmatch(r"[A-Za-z0-9-]{8,64}", header["run"]), "invalid audit nonce")
    identity = (uint(header.get("processPid"), "process PID", True), uint(header.get("processInstance"), "process QPC instance", True))
    require(identity[0] <= (1 << 32) - 1, "process PID outside Windows bounds")
    frequency = uint(header.get("qpcFrequency"), "QPC frequency", True)
    for key, maximum in dict(queueLimit=4096, recordLimit=65_536, byteLimit=MAX_FILE,
        aliasLimit=4096, aliasKeyLimit=1024, fieldLimit=16).items():
        require(0 < uint(header.get(key), key) <= maximum, "unsupported collector limit")
    worker = header["role"] == "worker"
    channel = uint(header.get("channel"), "worker channel", True) if worker else None
    if worker:
        uint(header.get("parentPid"), "parent PID", True)
        uint(header.get("parentInstance"), "parent QPC instance", True)
    else:
        require(all(header.get(key) is None for key in ("channel", "parentPid", "parentInstance")), "parent has worker-only identity fields")
    events, checkpoint, footer = [], None, None
    for row in rows[1:]:
        require(footer is None, "records follow a worker footer")
        kind = row.get("type")
        if kind == "event":
            require(row.get("kind") in SCHEMA, "unknown audit event kind")
            values = row.get("fields")
            require(isinstance(values, list) and len(values) == SCHEMA[row["kind"]] <= 16, "audit event arity mismatch")
            event = Event(row["kind"], tuple(uint(value, "event field") for value in values),
                uint(row.get("qpc"), "event QPC", True), uint(row.get("sequence"), "event sequence", True))
            require(event.sequence == len(events) + 1, "event sequence gap/replay")
            require(event.qpc >= identity[1], "event precedes process QPC instance")
            require(len(events) < 65_536, "record count bound exceeded")
            if worker:
                require(event.kind in ("RequestQueued", "RequestDequeued", "SubmitReturned", "WorkerOutcome", "QueueFault", "Fault"), "worker contains a parent-only event")
                if event.kind not in ("QueueFault", "Fault"):
                    require(event.f[0] == channel, "worker record channel differs from its header")
            else:
                require(event.kind not in ("SubmitReturned", "WorkerOutcome"), "parent contains worker-only native outcomes")
            events.append(event)
        elif kind in ("checkpoint", "footer"):
            report = validate_report(row, len(events), events[-1].sequence if events else 0, kind == "footer")
            if kind == "footer":
                require(worker and row.get("reason") in ("normal", "stop"), "missing normal worker finish")
                footer = (len(events), report)
            elif row.get("reason") == "stopped":
                checkpoint = (len(events), report)
        else:
            raise AuditError("unknown JSONL row type")
    selected = footer if worker else checkpoint
    require(selected is not None, "missing worker footer or valid stopped parent checkpoint")
    count, report = selected
    return Trace(header, events[:count], report, frequency, identity, channel, len(events) - count), len(data)


def load_traces(directory):
    paths = sorted(Path(directory).glob("*.jsonl"))
    require(1 < len(paths) <= 64, "audit directory needs one parent and bounded worker sidecars")
    traces, total = [], 0
    for path in paths:
        trace, size = parse_trace(path)
        total += size
        require(total <= MAX_TOTAL, "total audit input bound exceeded")
        traces.append(trace)
    require(len({trace.header["run"] for trace in traces}) == 1, "mixed audit run nonces")
    require(len({trace.frequency for trace in traces}) == 1, "QPC frequency mismatch")
    require(len({trace.identity for trace in traces}) == len(traces), "ambiguous process identities")
    parents = [trace for trace in traces if trace.header["role"] == "parent"]
    require(len(parents) == 1, "expected exactly one parent process")
    parent = parents[0]
    workers = {}
    opened = defaultdict(list)
    for event in parent.events:
        if event.kind == "ChannelOpened":
            opened[event.f[0]].append(event)
    for worker in (trace for trace in traces if trace is not parent):
        pair = (uint(worker.header["parentPid"]), uint(worker.header["parentInstance"]))
        require(pair == parent.identity, "worker parent identity mismatch")
        require(worker.channel not in workers and len(opened[worker.channel]) == 1, "ambiguous/missing worker channel")
        require(opened[worker.channel][0].f[1] == worker.identity[0], "worker PID does not match exact spawned channel")
        workers[worker.channel] = worker
    require(set(opened) == set(workers), "missing worker sidecar/footer for a spawned channel")
    return parent, workers


def nearest_rank(values, percentile):
    require(bool(values) and 1 <= percentile <= 100, "invalid percentile request")
    return sorted(values)[(len(values) * percentile + 99) // 100 - 1]


def latency(values, frequency):
    if not values:
        return None
    p95, p99 = nearest_rank(values, 95), nearest_rank(values, 99)
    return dict(samples=len(values), p95Ticks=str(p95), p99Ticks=str(p99),
        p95UsCeil=(p95 * 1_000_000 + frequency - 1) // frequency,
        p99UsCeil=(p99 * 1_000_000 + frequency - 1) // frequency,
        strictP95Below5ms=p95 * 1_000_000 < 5_000 * frequency,
        strictP99Below10ms=p99 * 1_000_000 < 10_000 * frequency)


def analyze(directory, phases, key_actions=(), key_ports=(), expected_key_sources=2, parent_report=None):
    parent, workers = load_traces(directory)
    require(parent_report is not None, "external returned parent checkpoint report is required")
    if isinstance(parent_report, (str, Path)):
        with Path(parent_report).open("rb") as stream:
            raw = stream.read(2 * 1024 * 1024 + 1)
        require(len(raw) <= 2 * 1024 * 1024, "external checkpoint report exceeds its bound")
        parent_report = json.loads(raw, object_pairs_hook=unique_json)
    if isinstance(parent_report, dict) and "ok" in parent_report:
        require(parent_report["ok"] is True, "external checkpoint request failed")
        parent_report = parent_report.get("state", {}).get("desktop", {}).get("audit")
    validate_report(dict(report=parent_report), len(parent.events), len(parent.events), False)
    require(parent_report["throughSequence"] == parent.report["throughSequence"], "external parent checkpoint prefix mismatch")
    require(phases and expected_key_sources > 0, "explicit workload phases and source count are required")
    by = defaultdict(list)
    for event in parent.events:
        by[event.kind].append(event)
    native_ids, native_changes, queue_epochs, typed_ids = set(), set(), defaultdict(set), defaultdict(set)
    for event in by["NativeEnqueue"]:
        f = event.f
        require(f[2] not in native_ids, "reused native frame audit identity")
        native_ids.add(f[2])
        change_key = (f[0], f[1], *f[3:7])
        require(change_key not in native_changes, "duplicated capture/change in one native queue")
        native_changes.add(change_key)
        queue_epochs[(f[0], f[3], f[4], f[6])].add((f[5], f[7], f[10]))
    require(all(len(values) == 1 for values in queue_epochs.values()), "native queue domain/generation/time changed within one admitted batch")
    for event in by["SyntheticFrame"]:
        require(event.f[0] not in native_ids, "reused synthetic/native frame identity")
        native_ids.add(event.f[0])
    for event in by["TypedAlias"]:
        f = event.f
        typed_ids[f[7]].add(tuple(f[:7]) + tuple(f[8:11]) + tuple(f[13:]))
    require(all(len(values) == 1 for values in typed_ids.values()), "typed audit frame identity was reused across conversions")
    errors = []
    def one(candidates, label):
        candidates = list(candidates)
        require(len(candidates) == 1, f"missing/ambiguous {label}")
        return candidates[0]
    def index(kind, key):
        result = defaultdict(list)
        for event in by[kind]:
            result[key(event.f)].append(event)
        return result
    captures = index("SourceCapture", lambda f: f[0])
    results = index("SourceResult", lambda f: f[0])
    effects = index("Effect", lambda f: f[0])
    roots = index("EffectRoot", lambda f: f[0])
    bindings = index("DeliveryBound", lambda f: (f[2], f[3], f[4]))
    # Native and typed sequences may reset. Keep provider/subscription/domain and
    # source-instance/run affiliation in every join; never pick the newest token.
    native = index("NativeEnqueue", lambda f: tuple(f[3:11]))
    dequeued = index("NativeDequeue", lambda f: tuple(f[:7]))
    typed = index("TypedAlias", lambda f: (f[8], f[11], f[9], f[10], f[13]))
    bounds = index("SourceBound", lambda f: (f[0], f[1], f[2], f[3]))
    requests = index("RequestIssued", lambda f: (f[4], f[5], f[2]))
    outcomes = index("ParentOutcome", lambda f: (f[0], f[1], f[2]))
    parent_queues = {kind: index(kind, lambda f: f[:2]) for kind in ("RequestQueued", "RequestDequeued")}
    worker_index = {}
    for channel, worker in workers.items():
        indexed = defaultdict(lambda: defaultdict(list))
        for event in worker.events:
            indexed[event.kind][event.f[:2] if event.kind in ("RequestQueued", "RequestDequeued") else event.f[:3]].append(event)
        worker_index[channel] = indexed
    discovered, chains = set(), {}
    chosen_actions = set(key_actions)
    for binding in by["DeliveryBound"]:
        if chosen_actions and binding.f[5] not in chosen_actions:
            continue
        try:
            effect = one(effects[binding.f[0]], "Effect")
            require(binding.f[:4] == effect.f[:4], "Effect/Delivery run or shadow mismatch")
            require(len(roots[effect.f[0]]) == effect.f[5] == 1, "only one immutable input root is eligible")
            root = roots[effect.f[0]][0]
            rf = root.f
            candidates = []
            for alias in typed[(rf[1], rf[2], rf[3], rf[5], rf[6])]:
                af = alias.f
                associated = bounds[(af[0], effect.f[2], effect.f[3], af[8])]
                matching_bounds = [bound for bound in associated if bound.f[4:7] == (af[1], af[2], af[4])]
                if matching_bounds:
                    one(matching_bounds, "SourceBound affiliation")
                    candidates.append(alias)
            alias = one(candidates, "run-bound TypedAlias for immutable EffectRoot")
            af = alias.f
            require(af[14] == 0, "synthetic-root")
            frame = one(native[(af[1], af[2], af[3], af[4], af[5], af[6], af[12], af[13])], "native frame including queue domain")
            nf = frame.f
            one(dequeued[(nf[2], *nf[3:9])], "native frame dequeue")
            capture = one(captures[nf[0]], "source capture")
            result = one(results[nf[0]], "source admission result")
            require(result.f[1:] == (1, 0), "source input was not admitted")
            require(capture.f[1:3] == (nf[3], nf[6]) and nf[1] < capture.f[7], "capture source/change-index mismatch")
            pair = (nf[6], nf[9])
            if key_ports and nf[9] not in key_ports:
                continue
            discovered.add(pair)
            chains[id(binding)] = (effect, root, alias, frame, capture)
        except AuditError as error:
            # Synthetic baseline/renewal paths are explicitly outside admission
            # latency. They are counted; all other unresolved bindings fail.
            if str(error) != "synthetic-root":
                errors.append(str(error))
    require(discovered, "no selected real KeyHold source/port chains")
    selected_actions = chosen_actions or {event.f[5] for event in by["DeliveryBound"] if id(event) in chains}
    require(len({source for source, _ in discovered}) == expected_key_sources and len(selected_actions) == expected_key_sources,
        "selected key source/action count mismatch")
    phase_results = []
    intervals = []
    for phase in phases:
        require(phase.expected_edges > 0, "expected key edges must be positive")
        def marker(code, identity):
            return one((event for event in by["Marker"] if event.f[1] == code and
                (identity is None or event.f[0] == identity)), "phase marker")
        begin, end = marker(phase.begin, phase.begin_id if phase.begin_id is not None else phase.marker_id), marker(phase.end, phase.end_id if phase.end_id is not None else phase.marker_id)
        require(begin.qpc < end.qpc, "phase markers are reversed/ambiguous")
        require(not any(begin.qpc < prior_end and end.qpc > prior_begin for prior_begin, prior_end in intervals), "overlapping workload phases")
        intervals.append((begin.qpc, end.qpc))
        excluded = Counter({key: 0 for key in EXCLUDED})
        samples, joined, target, source_set = [], set(), set(), set()
        phase_errors = []
        phase_captures = [event for event in by["SourceCapture"] if begin.qpc < event.qpc < end.qpc]
        for capture in phase_captures:
            result = one(results[capture.f[0]], "phase source result")
            if result.f[1:] != (1, 0):
                phase_errors.append("phase contains a rejected native admission")
        for frame in by["NativeEnqueue"]:
            nf = frame.f
            capture = one(captures[nf[0]], "native enqueue capture")
            if (nf[6], nf[9]) in discovered and begin.qpc < capture.qpc < end.qpc:
                target.add((nf[0], nf[1], nf[6], nf[9]))
        for binding in by["DeliveryBound"]:
            chain = chains.get(id(binding))
            if chain is None:
                continue
            effect, root, alias, frame, capture = chain
            if not begin.qpc < capture.qpc < end.qpc:
                continue
            nf = frame.f
            target_key = (nf[0], nf[1], nf[6], nf[9])
            try:
                require(target_key not in joined, "one selected key change has multiple deliveries")
                joined.add(target_key)
                source_set.add(nf[6])
                require(len(bindings[tuple(binding.f[2:5])]) == 1, "ambiguous delivery operation")
                request = one((event for event in requests[tuple(binding.f[2:5])] if event.f[3] == 3), "run-bound delivery request")
                qf = request.f
                worker = workers.get(qf[0])
                require(worker is not None, "missing worker for exact channel")
                key = (qf[0], qf[1], qf[2])
                queue_points = []
                for indexed in (parent_queues, worker_index[qf[0]]):
                    for kind in ("RequestQueued", "RequestDequeued"):
                        queue_points.append(one(indexed[kind].get(key[:2], []), "exact transport queue record"))
                worker_outcome = one(worker_index[qf[0]]["WorkerOutcome"].get(key, []), "worker acknowledgement")
                parent_outcome = one(outcomes[key], "validated parent acknowledgement")
                wf, pf = worker_outcome.f, parent_outcome.f
                require(wf[3:6] == pf[3:6] and wf[3] <= 31 and 0 <= wf[5] <= wf[4] <= 64, "parent/worker ACK mismatch")
                flags, attempted, confirmed = wf[3:6]
                submitted = [event for event in worker_index[qf[0]]["SubmitReturned"].get(key, []) if event.f[3] == 1]
                if attempted:
                    native_send = one(submitted, "native submission outcome")
                    require(native_send.f[4:] == (attempted, confirmed), "SendInput/ACK count mismatch")
                else:
                    require(not submitted, "zero-count ACK has a native submission")
                if not flags & OK or flags & UNCERTAIN or pf[6] or attempted != confirmed:
                    excluded["fault"] += 1
                elif flags & SKIPPED:
                    excluded["skipped"] += 1
                elif flags & COLLISION:
                    excluded["collision"] += 1
                elif attempted == 0:
                    excluded["noop"] += 1
                else:
                    send = one(submitted, "successful exact SendInput return")
                    require(send.f[4:] == (attempted, confirmed), "SendInput/ACK count mismatch")
                    require(attempted == 1, "selected KeyHold must produce exactly one native record")
                    require(flags & CLEAN, "successful ACK has incomplete cleanup status")
                    causal = [capture, frame, alias, effect, binding, request, *queue_points, send, worker_outcome, parent_outcome]
                    require(all(left.qpc <= right.qpc for left, right in zip(causal, causal[1:])), "causal QPC order violation")
                    require(send.qpc - capture.qpc > 1, "ambiguous admission-to-SendInput QPC interval (one tick or less)")
                    samples.append(send.qpc - capture.qpc)
            except AuditError as error:
                excluded["unacked"] += 1
                phase_errors.append(str(error))
        missing = target - joined
        excluded["unresolved"] += len(missing)
        if missing:
            phase_errors.append("selected native key changes have no exact Effect/Delivery/ACK chain")
        phase_events = [event for event in parent.events if begin.qpc < event.qpc < end.qpc]
        worker_phase_events = [event for worker in workers.values() for event in worker.events if begin.qpc < event.qpc < end.qpc]
        if any(event.kind in ("Fault", "QueueFault", "NativeGap") for event in phase_events + worker_phase_events):
            phase_errors.append("phase contains native fault/queue loss")
        for worker in workers.values():
            excluded["cleanup"] += sum(event.kind == "SubmitReturned" and event.f[3] != 1 and begin.qpc < event.qpc < end.qpc for event in worker.events)
        excluded["synthetic"] += sum(event.kind == "SyntheticFrame" for event in phase_events)
        phase_errors.extend(errors)
        measured = latency(samples, parent.frequency) if not phase_errors else None
        counts_match = len(target) == len(samples) == phase.expected_edges and len(source_set) == expected_key_sources
        phase_results.append(dict(name=phase.name, expectedKeyEdges=phase.expected_edges,
            admittedBatches=len(phase_captures), admittedChanges=sum(event.f[7] for event in phase_captures),
            selectedKeyChanges=len(target), successfulKeyEdges=len(samples), keySources=len(source_set),
            excluded=dict(excluded), droppedDeliveries=sum(event.kind == "DeliveryDropped" for event in phase_events),
            nativeGaps=sum(event.kind == "NativeGap" for event in phase_events), queueFaults=sum(event.kind == "QueueFault" for event in phase_events + worker_phase_events),
            queues=dict(nativeQueuedFramesMax=max((event.f[11] for event in phase_events if event.kind == "NativeEnqueue"), default=0),
                parentOutstandingReservationsMax=max((event.f[2] for event in phase_events if event.kind == "RequestQueued"), default=0),
                workerOutstandingReservationsMax=max((event.f[2] for worker in workers.values() for event in worker.events if event.kind == "RequestQueued" and begin.qpc < event.qpc < end.qpc), default=0)),
            latency=measured, errors=sorted(set(phase_errors)), expectedCountsMatch=counts_match,
            passed=bool(measured and measured["strictP95Below5ms"] and measured["strictP99Below10ms"] and counts_match and not any(excluded[key] for key in ("noop", "collision", "skipped", "cleanup", "unacked", "fault", "unresolved")))))
    # Each exact worker must have a successfully acknowledged Stop, independent
    # of latency endpoints. A footer alone never proves native ownership cleanup.
    for channel, worker in workers.items():
        exited = one((event for event in by["ChannelExited"] if event.f[0] == channel), "exact worker exit proof")
        require(exited.f[1:] == (worker.identity[0], 0, 1, 1), "worker audit finish/exit was not graceful and successful")
        stop = one((event for event in by["RequestIssued"] if event.f[0] == channel and event.f[3] == 7), "worker Stop request")
        key = (channel, stop.f[1], 0)
        parent_stop = one(outcomes[key], "parent Stop acknowledgement")
        worker_stop = one((event for event in worker.events if event.kind == "WorkerOutcome" and event.f[:3] == key), "worker Stop acknowledgement")
        require(parent_stop.f[3:6] == worker_stop.f[3:6] and parent_stop.f[6] == 0 and parent_stop.f[3] == OK | CLEAN, "worker Stop was not proven successful/clean")
        require(parent_stop.qpc <= exited.qpc, "worker exit precedes acknowledged Stop")
    return dict(schemaVersion=1, measurement="Native authenticated WSS SourceSink admission to successful SendInput return",
        physicalCaptureMeasured=False, qpcFrequency=str(parent.frequency), traceValid=all(not phase["errors"] for phase in phase_results),
        passed=all(phase["passed"] for phase in phase_results), selectedKeyActions=sorted(selected_actions),
        selectedPhysicalPorts=[dict(source=source, port=port) for source, port in sorted(discovered)],
        queueMetric="Transport depths are outstanding reservations, not atomic occupancy; native depth counts queued frames.",
        parentIgnoredTailEvents=parent.ignored_tail, phases=phase_results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audit-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--parent-report", type=Path, required=True, help="Returned stopped checkpoint FlushReport or full native IPC response")
    parser.add_argument("--phase", action="append", required=True, metavar="NAME:BEGIN:END:EXPECTED[:BEGIN_ID:END_ID]")
    parser.add_argument("--marker-id", type=int)
    parser.add_argument("--key-actions", default="", help="Comma-separated numeric action aliases; otherwise discover KeyHold bindings")
    parser.add_argument("--key-ports", default="", help="Comma-separated physical-port aliases; source identity remains part of every key")
    parser.add_argument("--expected-key-sources", type=int, default=2)
    args = parser.parse_args()
    try:
        phases = []
        for value in args.phase:
            parts = value.split(":")
            require(len(parts) in (4, 6), "phase requires name, phase codes, expected edges and optional exact marker IDs")
            name, begin, end, expected = parts[:4]
            identities = [int(number) for number in parts[4:]] if len(parts) == 6 else [None, None]
            phases.append(Phase(name, int(begin), int(end), int(expected), args.marker_id, *identities))
        aliases = lambda value: [int(number) for number in value.split(",") if number]
        result = analyze(args.audit_dir, phases, aliases(args.key_actions), aliases(args.key_ports), args.expected_key_sources, args.parent_report)
    except (AuditError, OSError, ValueError, TypeError, KeyError) as error:
        result = dict(schemaVersion=1, measurement="Native WSS admission to SendInput return", passed=False, traceValid=False, errors=[str(error)])
    payload = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload, encoding="utf-8")
    print(payload, end="")
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
