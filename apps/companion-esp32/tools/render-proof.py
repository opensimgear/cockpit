#!/usr/bin/env python3
"""Summarize sanitized render-poc JSONL and optional full application readbacks.

Reads files only; never opens a port or emits raw status, host errors, credentials,
SSID, input values or installer NVS data. Durations cover consecutive observed
live-armed samples, not unsampled time or hardware qualification.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import sys

MAX_LINE = 8192
MAX_FILE = 16 * 1024 * 1024
MAX_COUNTER = 18446744073709551615
MAX_UINT32 = 4294967295
SERIAL_HEALTH = (
    "fifoOverflows", "bufferFull", "frameErrors", "parityErrors", "breaks",
    "queueFullObservations", "requestTimeouts", "invalidJson", "replyMismatch",
    "deferredFull", "invalidReply", "writeFailures", "frameOverflows",
)
ACK_FIELDS = ("startupAckMaxBytes", "steadyAckMaxBytes")
INPUT_FIELDS = ("baselines", "sourceLosses", "inputBatches")
# Exact known firmware literals, including older bench firmware's combined ACK
# failure. Unknown/free-form statuses are never echoed into a proof artifact.
EVENT_STATUSES = frozenset("""
offline pairing awaiting-approval authenticating awaiting-assignment opening-panel
live-disarmed error live-armed arming stopped paired awaiting-host-clock
downloading-render-package presenting-render-package opening-render-session
render-package-verified render-session-disarmed choose-or-assign-one-instance-in-cockpit
host-rejected-or-invalid-reply-stop-and-reconnect invalid-command-json
source-transport-failed-reconnect-required source-host-rejected-reconnect-required
source-ack-invalid-reconnect-required source-ack-input-unhealthy-reconnect-required
baseline-not-confirmed-or-touch-changed-reconnect-required host-tls-or-connection-failed
input-backlog-or-counter-invalid-reconnect-required input-cancelled-or-lease-expired-reconnect-required
invalid-installation-id pairing-approval-expired pairing-offer-expired
panel-unsupported-corrupt-or-over-budget render-engine-could-not-suspend
render-package-first-presentation-failed render-package-logical-panel-rejected
serial-command-fragment-timeout-input-stopped session-counter-exhausted setup-delete-failed
setup-save-failed-repair-required setup-storage-unavailable trusted-clock-invalid
trusted-clock-or-saved-installation-unavailable wifi-connection-failed approve-pairing-in-cockpit
arm-rejected-release-touch-check-display-and-leases arm-rejected-stale-or-missing-session
cached-render-package-restored-offline cached-render-panel-rejected diagnostics-requires-stopped-input
diagnostics-reset disarmed-reconnect-required invalid-provisioning-record network-task-allocation-failed
pair-first-or-invalid-instance provisioning-line-too-large refreshing-cockpit-assignment-disarmed
render-storage-initialization-suspend-failed restoring-saved-setup saved-setup-clock-unavailable
saved-setup-wifi-unavailable saved-wifi-password-unavailable scene-page-changed-input-stopped
serial-command-fragment-expired serial-driver-initialization-failed serial-provisioning-ready
unsupported-command page-changed invalid-page forgotten render-main-not-suspended
render-package-transfer-failed render-package-transfer-or-validation-failed
""".split())
BOARD_PROFILES = {
    "sunton": ("sunton-3248s035r", "sunton-3248s035r-render-v1"),
    "waveshare": ("waveshare-lcd7-s3", "waveshare-lcd7-s3-render-v1"),
}
HASH = re.compile(r"sha256:[0-9a-f]{64}\Z")
RAW_HASH = re.compile(r"[0-9a-f]{64}\Z")
ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}\Z")
DECIMAL = re.compile(r"(?:0|[1-9][0-9]{0,19})\Z")
FIRMWARE_VERSION = re.compile(r"(?:20[0-9]{2}\.[0-9]{2}\.[0-9]{2}\.[0-9]{1,6}|[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3})\Z")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def counter(value):
    require(type(value) is int and 0 <= value <= MAX_COUNTER, "Invalid evidence counter")
    return value


def uint32(value):
    require(counter(value) <= MAX_UINT32, "Invalid uint32 evidence counter")
    return value


def health_counters(value):
    require(isinstance(value, dict) and set(value) == set(SERIAL_HEALTH),
            "Invalid serial health counter shape")
    return {key: uint32(value[key]) for key in SERIAL_HEALTH}


def digest(value, raw=False):
    require(isinstance(value, str) and (RAW_HASH if raw else HASH).fullmatch(value), "Invalid evidence SHA-256")
    return value


def object_pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "Duplicate JSON field")
        result[key] = value
    return result


def finite_number(value):
    number = float(value)
    require(math.isfinite(number), "Non-finite JSON number")
    return number


def invalid_constant(_):
    raise ValueError("Non-finite JSON constant")


def decode(raw):
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=object_pairs,
                           parse_float=finite_number, parse_constant=invalid_constant)
    except (UnicodeError, RecursionError, ValueError):
        raise ValueError("Malformed evidence JSON") from None
    require(isinstance(value, dict), "Evidence record must be an object")
    return value


def read_records(path, allow_running=False):
    """Enforce both limits while reading, including a file that grows mid-read."""
    require(Path(path).stat().st_size <= MAX_FILE, "Evidence file exceeds 16 MiB")
    total = 0
    with Path(path).open("rb") as source:
        while True:
            raw = source.readline(MAX_LINE + 1)
            if not raw:
                break
            total += len(raw)
            require(total <= MAX_FILE, "Evidence file exceeds 16 MiB")
            require(len(raw) <= MAX_LINE, "Evidence line exceeds 8192 bytes")
            if not raw.endswith(b"\n"):
                require(allow_running, "Unterminated final evidence line")
                # Only an unfinished final write may be ignored. Any bytes after
                # this bounded fragment still reject rather than truncate input.
                require(not source.read(1), "Evidence line exceeds 8192 bytes")
                break
            yield decode(raw)


def read_object(path):
    with Path(path).open("rb") as source:
        raw = source.read(MAX_LINE + 1)
    require(len(raw) <= MAX_LINE, "Evidence JSON exceeds 8192 bytes")
    return decode(raw)


def load_identities(root):
    """Bind fixture IDs to exact manifest bytes; no large artwork reads needed."""
    catalog = {}
    for board, (board_profile, profile_id) in BOARD_PROFILES.items():
        hashes = set()
        for revision in ("A", "B"):
            directory = Path(root) / board / revision
            identity = read_object(directory / "identity.json")
            with (directory / "manifest.json").open("rb") as source_file:
                raw = source_file.read(MAX_LINE + 1)
            require(len(raw) <= MAX_LINE, "Fixture manifest exceeds 8192 bytes")
            manifest = decode(raw)
            package = digest(identity.get("packageHash"))
            source = digest(identity.get("sourcePublicationHash"))
            profile = digest(identity.get("profileHash"))
            require(package == "sha256:" + hashlib.sha256(raw).hexdigest(), "Fixture manifest hash mismatch")
            require(manifest.get("sourcePublicationHash") == source and
                    manifest.get("targetProfileHash") == profile and
                    manifest.get("targetProfileId") == profile_id, "Fixture source/profile mismatch")
            require(package not in catalog, "Duplicate fixture package")
            catalog[package] = dict(board=board, revision=revision, boardProfile=board_profile,
                                    targetProfileId=profile_id, profileHash=profile,
                                    packageHash=package, sourcePublicationHash=source)
            hashes.add(profile)
        require(len(hashes) == 1, "Board revisions have different profile identities")
    return catalog


def port_key(value):
    require(isinstance(value, str) and 0 < len(value) <= 128 and
            all(32 <= ord(c) < 127 for c in value), "Invalid evidence connection key")
    return value


def installer_record(value):
    profile = value.get("profile")
    boards = [board for board, (known, _) in BOARD_PROFILES.items() if profile == known]
    require(len(boards) == 1, "Unknown installer board profile")
    application_hash = digest(value.get("applicationSha256"), raw=True)
    require(digest(value.get("expectedApplicationSha256"), raw=True) == application_hash,
            "Installer application hash differs from its expected full image")
    size = counter(value.get("applicationBytes"))
    require(0 < size <= 1536 * 1024, "Invalid installer application byte range")
    require(type(value.get("readOnly")) is bool, "Missing installer read-only state")
    segments = value.get("segments")
    require(isinstance(segments, list) and len(segments) <= 8, "Invalid installer segment list")
    applications = [s for s in segments if isinstance(s, dict) and s.get("name") == "application"]
    require(len(applications) == 1, "Missing unique full application segment")
    segment = applications[0]
    require(segment.get("offset") == 65536 and type(segment.get("offset")) is int and
            segment.get("bytes") == size and type(segment.get("bytes")) is int and
            segment.get("sha256") == application_hash and
            segment.get("expectedSha256") == application_hash, "Installer full application range/hash mismatch")
    offsets = value.get("writtenOffsets")
    require(isinstance(offsets, list) and len(offsets) <= 8 and
            all(type(n) is int and 0 <= n < 16 * 1024 * 1024 for n in offsets), "Invalid installer write offsets")
    require(not value["readOnly"] or not offsets, "Read-only installer record contains writes")
    return boards[0], dict(sha256=application_hash, bytes=size, readOnly=value["readOnly"],
                          port=port_key(value.get("port")))


def load_installers(paths):
    result = {}
    for path in paths:
        board, record = installer_record(read_object(path))
        require(board not in result, "Duplicate installer evidence for board")
        result[board] = record
    return result


def application_proof(board, port, before, after):
    first, last = before.get(board), after.get(board)
    result = {"status": "not-provided", "unchangedVerified": False}
    if first:
        result["beforeSha256"], result["beforeBytes"] = first["sha256"], first["bytes"]
    if last:
        require(last["readOnly"], "After evidence must be a read-only full application check")
        result["afterSha256"], result["afterBytes"] = last["sha256"], last["bytes"]
    if first is None and last is not None:
        result["status"] = "missing-before"
    elif first is not None and last is None:
        result["status"] = "missing-after"
    elif first is not None and last is not None:
        if first["port"] != last["port"] or (port is not None and first["port"] != port):
            result["status"] = "connection-mismatch"
        elif first["sha256"] == last["sha256"] and first["bytes"] == last["bytes"]:
            result.update(status="unchanged", unchangedVerified=True)
        else:
            result["status"] = "changed"
    return result


def summarize(records, catalog, before=None, after=None, allow_running=False, max_gap_ms=2500):
    require(type(max_gap_ms) is int and 0 < max_gap_ms <= 60000, "Invalid maximum sample gap")
    before, after = before or {}, after or {}
    board_by_profile = {profile: board for board, (profile, _) in BOARD_PROFILES.items()}
    states, ports = {}, {}
    complete = False
    counts, instance_counts = {}, {}
    pending_events, event_cursors = {}, {}
    record_count = 0

    def state(board):
        if board not in states:
            identities = [v for v in catalog.values() if v["board"] == board]
            require(len(identities) == 2, "Missing board fixture identities")
            states[board] = dict(port=None, previous=None, current=None, lastTime=None,
                                lastBytes=None, lastReads=None, sessions=set(), events=[],
                                pairs=0, byteIncrement=0, readIncrement=0, regressions=0,
                                missing=0, rejected=0, deviceEvents=[], deviceEventCounts={},
                                eventGaps=0, health=None, previousHealth=None, healthSamples=0,
                                healthRegressions=0, ack={key: None for key in ACK_FIELDS},
                                firstLiveHealth=None, firmwareVersions=set(),
                                liveHealthMeasured=0, liveHealthMissing=0, liveHealthResetPairs=0,
                                liveHealthIncrements={key: 0 for key in SERIAL_HEALTH},
                                liveHealthRegressions={key: 0 for key in SERIAL_HEALTH})
        return states[board]

    def apply_event(board, entry):
        current = state(board)
        require(current["lastTime"] is None or entry["elapsedMs"] >= current["lastTime"],
                "Device event precedes an already observed sample")
        require(len(current["deviceEvents"]) < 10000, "Device event report budget")
        current["deviceEvents"].append({key: entry[key] for key in
                                       ("sequence", "elapsedMs", "transportElapsedMs", "status")})
        status = entry["status"]
        current["deviceEventCounts"][status] = current["deviceEventCounts"].get(status, 0) + 1
        current["eventGaps"] += entry["gap"]
        if status != "live-armed":
            current["previous"] = current["current"] = None

    for value in records:
        record_count += 1
        require(not complete, "Evidence continues after complete event")
        event = value.get("event")
        if event == "published":
            board, revision = value.get("board"), value.get("revision")
            require(board in BOARD_PROFILES and revision in ("A", "B"), "Unknown published board/revision")
            package = digest(value.get("packageHash"))
            source = digest(value.get("publicationHash"))
            identity = catalog.get(package)
            require(identity is not None and identity["board"] == board and
                    identity["revision"] == revision and identity["sourcePublicationHash"] == source,
                    "Published identities differ from compiled fixture")
            state(board)
        elif event == "device-event":
            connection = port_key(value.get("port"))
            require(value.get("status") in EVENT_STATUSES, "Unknown device event status")
            sequence, elapsed, transport = (counter(value.get(key)) for key in
                                            ("sequence", "elapsedMs", "transportElapsedMs"))
            require(sequence > 0, "Invalid device event sequence")
            previous = event_cursors.get(connection)
            require(previous is None or (sequence > previous[0] and elapsed >= previous[1]
                    and transport >= previous[2]), "Device event sequence/time regressed")
            event_cursors[connection] = (sequence, elapsed, transport)
            entry = dict(sequence=sequence, elapsedMs=elapsed, transportElapsedMs=transport,
                         status=value["status"], gap=sequence - (previous[0] if previous else 0) - 1)
            if connection in ports:
                apply_event(ports[connection], entry)
            else:
                require(connection in pending_events or len(pending_events) < 8,
                        "Unattributed event connection budget")
                pending = pending_events.setdefault(connection, [])
                require(len(pending) < 256, "Unattributed event budget")
                pending.append(entry)
        elif event == "device":
            now = counter(value.get("elapsedMs"))
            connection = port_key(value.get("port"))
            ack_measurements = {key: uint32(value[key]) for key in ACK_FIELDS if value.get(key) is not None}
            for key in ("contentBytes", "contentRequests"):
                if value.get(key) is not None:
                    counter(value[key])
            device = value.get("device")
            require(device is None or isinstance(device, dict), "Invalid device record")
            firmware = device.get("firmwareVersion") if device else None
            if firmware is not None:
                require(isinstance(firmware, str) and FIRMWARE_VERSION.fullmatch(firmware),
                        "Invalid firmware release version")
            board = ports.get(connection)
            if device is not None:
                profile = device.get("boardProfile")
                require(profile in board_by_profile, "Unknown device board profile")
                known = board_by_profile[profile]
                require(board is None or board == known, "Board profile changed on one connection")
                board = known
                current = state(board)
                require(current["port"] in (None, connection), "Multiple connections claim one board profile")
                current["port"] = connection
                ports[connection] = board
                for entry in pending_events.pop(connection, []):
                    apply_event(board, entry)
                for key in ("renderPackageHash", "publicationHash"):
                    if device.get(key) not in (None, ""):
                        digest(device[key])
            if board is None:
                continue
            current = state(board)
            if firmware is not None:
                current["firmwareVersions"].add(firmware)
            require(current["lastTime"] is None or now >= current["lastTime"], "Device evidence time regressed")
            current["lastTime"] = now
            content, reads = value.get("contentBytes"), value.get("contentRequests")
            if content is not None:
                current["lastBytes"] = counter(content)
            if reads is not None:
                current["lastReads"] = counter(reads)
            for key, measured in ack_measurements.items():
                current["ack"][key] = max(current["ack"][key] or 0, measured)
            sample_health = None
            if device is not None and device.get("serialHealth") is not None:
                measured = health_counters(device["serialHealth"])
                sample_health = measured.copy()
                current["healthSamples"] += 1
                if current["previousHealth"] is not None:
                    current["healthRegressions"] += sum(measured[key] < current["previousHealth"][key]
                                                         for key in SERIAL_HEALTH)
                current["health"] = {key: max((current["health"] or {}).get(key, 0), measured[key])
                                     for key in SERIAL_HEALTH}
                current["previousHealth"] = measured
            live = device is not None and device.get("status") == "live-armed" and \
                value.get("deviceStatus") == "live-armed" and value.get("lastHostError") in (None, "")
            if not live:
                current["previous"] = current["current"] = None
                continue
            identity = catalog.get(device.get("renderPackageHash"))
            require(identity is not None and identity["board"] == board and
                    identity["sourcePublicationHash"] == device.get("publicationHash"), "Live package/source differs from fixture")
            session, generation = device.get("sessionId"), device.get("generation")
            require(isinstance(session, str) and ID.fullmatch(session), "Invalid live session identity")
            require(isinstance(generation, str) and DECIMAL.fullmatch(generation) and
                    int(generation) <= MAX_COUNTER, "Invalid live session generation")
            current["sessions"].add((session, generation))
            if current["firstLiveHealth"] is None:
                current["firstLiveHealth"] = dict(elapsedMs=now, packageHash=identity["packageHash"],
                    sourcePublicationHash=identity["sourcePublicationHash"], sessionId=session,
                    generation=generation, firmwareVersion=firmware, counters=sample_health)
            live_key = (identity["packageHash"], session, generation)
            previous = current["previous"]
            consecutive = previous is not None and previous["key"] == live_key
            if consecutive:
                current["pairs"] += 1
                old_health = previous["health"]
                if sample_health is None or old_health is None:
                    current["liveHealthMissing"] += 1
                else:
                    regressed = [key for key in SERIAL_HEALTH if sample_health[key] < old_health[key]]
                    if regressed:
                        # A reset makes this entire pair ambiguous: never add a
                        # positive difference from another counter in that pair.
                        current["liveHealthResetPairs"] += 1
                        for key in regressed:
                            current["liveHealthRegressions"][key] += 1
                    else:
                        current["liveHealthMeasured"] += 1
                        for key in SERIAL_HEALTH:
                            current["liveHealthIncrements"][key] += sample_health[key] - old_health[key]
                if None in (content, reads, previous["bytes"], previous["reads"]):
                    current["missing"] += 1
                else:
                    delta, read_delta = content - previous["bytes"], reads - previous["reads"]
                    current["regressions"] += int(delta < 0 or read_delta < 0)
                    current["byteIncrement"] += max(0, delta)
                    current["readIncrement"] += max(0, read_delta)
            if not consecutive or now - previous["time"] > max_gap_ms:
                segment = {key: identity[key] for key in ("board", "revision", "boardProfile", "targetProfileId", "profileHash", "packageHash", "sourcePublicationHash")}
                segment.update({"sessionId": session, "generation": generation,
                           "firstObservedMs": now, "lastObservedMs": now, "samples": 1,
                           "firmwareVersion": firmware, "firstSerialHealth": sample_health})
                current["events"].append(segment)
                current["current"] = segment
            else:
                current["current"]["lastObservedMs"] = now
                current["current"]["samples"] += 1
            current["previous"] = dict(key=live_key, bytes=content, reads=reads, time=now, health=sample_health)
        elif event == "feedback-rejected":
            board = value.get("board")
            require(board in BOARD_PROFILES, "Unknown rejected-feedback board")
            state(board)["rejected"] += 1
        elif event in ("input-evidence", "complete"):
            if event == "input-evidence":
                counter(value.get("elapsedMs"))
            raw_counts = value.get("counts")
            require(raw_counts is None or isinstance(raw_counts, dict), "Invalid input evidence counts")
            if raw_counts:
                for key in INPUT_FIELDS:
                    if key in raw_counts:
                        counts[key] = counter(raw_counts[key])
                instances = raw_counts.get("instances")
                if instances is not None:
                    require(isinstance(instances, dict) and len(instances) <= 32,
                            "Invalid per-instance input evidence")
                    for board in BOARD_PROFILES:
                        instance = "render-poc-" + board
                        if instance in instances:
                            item = instances[instance]
                            require(isinstance(item, dict), "Invalid instance counters")
                            stored = instance_counts.setdefault(instance, {})
                            for key in INPUT_FIELDS:
                                if key in item:
                                    stored[key] = counter(item[key])
            if event == "complete":
                complete = True
        else:
            raise ValueError("Unknown proof event")
    require(record_count > 0, "Empty proof evidence")
    require(complete or allow_running, "Missing complete event")
    reports = []
    for board in sorted(states):
        current = states[board]
        sample = next(v for v in catalog.values() if v["board"] == board)
        longest = max((e["lastObservedMs"] - e["firstObservedMs"] for e in current["events"]), default=0)
        measured_pairs = current["pairs"] - current["missing"]
        violation = current["byteIncrement"] != 0 or current["readIncrement"] != 0 or current["regressions"] != 0
        verified = False if violation else (None if measured_pairs == 0 or current["missing"] else True)
        health_positive = any(current["liveHealthIncrements"].values())
        health_zero = False if health_positive else (None if current["liveHealthMeasured"] == 0 or
            current["liveHealthMissing"] or current["liveHealthResetPairs"] else True)
        reports.append(dict(board=board, boardProfile=sample["boardProfile"], targetProfileId=sample["targetProfileId"],
                            profileHash=sample["profileHash"], liveEvents=current["events"],
                            sessionCount=len(current["sessions"]), longestContinuousLiveMs=longest,
                            lastContentBytes=current["lastBytes"], lastContentRequests=current["lastReads"],
                            consecutiveLiveContent=dict(pairs=current["pairs"], measuredPairs=measured_pairs,
                                byteIncrement=current["byteIncrement"] if measured_pairs else None,
                                requestIncrement=current["readIncrement"] if measured_pairs else None, counterRegressions=current["regressions"],
                                missingCounterPairs=current["missing"], allZero=verified),
                            feedbackRejectedCount=current["rejected"],
                            deviceEvents=current["deviceEvents"], deviceEventCounts=current["deviceEventCounts"],
                            deviceEventSequenceGaps=current["eventGaps"],
                            startupAckMaxBytes=current["ack"]["startupAckMaxBytes"],
                            steadyAckMaxBytes=current["ack"]["steadyAckMaxBytes"],
                            serialHealthMax=current["health"], serialHealthSamples=current["healthSamples"],
                            serialHealthCounterRegressions=current["healthRegressions"],
                            firmwareVersions=sorted(current["firmwareVersions"]) or None,
                            firstLiveSerialHealth=current["firstLiveHealth"],
                            consecutiveLiveSerialHealth=dict(pairs=current["pairs"],
                                measuredPairs=current["liveHealthMeasured"],
                                missingCounterPairs=current["liveHealthMissing"],
                                counterRegressionPairs=current["liveHealthResetPairs"],
                                counterRegressionsByField=current["liveHealthRegressions"],
                                increments={key: current["liveHealthIncrements"][key]
                                    if current["liveHealthMeasured"] else None for key in SERIAL_HEALTH},
                                allZero=health_zero),
                            inputEvidenceCounts=instance_counts.get("render-poc-" + board),
                            application=application_proof(board, current["port"], before, after)))
    return dict(formatVersion=1, runComplete=complete, liveStatus="live-armed", maximumSampleGapMs=max_gap_ms,
                records=record_count, boards=reports, inputEvidenceCounts=counts,
                inputEvidenceByInstance=instance_counts,
                unattributedDeviceEventCount=sum(len(v) for v in pending_events.values()),
                scope="Consecutive sampled runtime and full application readback evidence only.",
                notEstablished=["visual quality", "soak qualification", "power-cut recovery",
                                "absence of unobserved serial failures or lost queue events"])


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    parser.add_argument("--fixtures", type=Path, default=Path(__file__).resolve().parents[3] / "packages/companion-renderer/test/poc")
    parser.add_argument("--installer-before", action="append", type=Path, default=[])
    parser.add_argument("--installer-after", action="append", type=Path, default=[])
    parser.add_argument("--allow-running", action="store_true")
    parser.add_argument("--max-gap-ms", type=int, default=2500)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    try:
        report = summarize(read_records(args.log, args.allow_running), load_identities(args.fixtures),
                           load_installers(args.installer_before), load_installers(args.installer_after),
                           args.allow_running, args.max_gap_ms)
        result = json.dumps(report, indent=2, sort_keys=True, ensure_ascii=True) + "\n"
        if args.output:
            args.output.write_text(result, encoding="utf-8", newline="\n")
        else:
            sys.stdout.write(result)
    except (ValueError, OSError):
        # Never interpolate rejected input, filenames or JSON into diagnostics.
        print("Cannot summarize: invalid, incomplete or unavailable proof evidence.", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())