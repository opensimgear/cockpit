"""Portable synthetic regressions: these fixtures perform no native operations."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("desktop_latency", Path(__file__).with_name("desktop-latency.py"))
audit = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = audit
spec.loader.exec_module(audit)


def event(kind, fields, at):
    return dict(type="event", kind=kind, fields=[str(value) for value in fields], qpc=str(at))


def header(role):
    return dict(type="header", version="1", run="synthetic-run", role=role,
        processPid="10" if role == "parent" else "20", processInstance="100" if role == "parent" else "200",
        parentPid=None if role == "parent" else "10", parentInstance=None if role == "parent" else "100",
        channel=None if role == "parent" else "7", qpcFrequency="1000000", queueLimit="4096", recordLimit="65536",
        byteLimit=str(audit.MAX_FILE), aliasLimit="4096", aliasKeyLimit="1024", fieldLimit="16")


def complete(rows, role):
    rows = copy.deepcopy(rows)
    for sequence, value in enumerate(rows, 1):
        value["sequence"] = str(sequence)
    counters = {key: "0" for key in audit.COUNTERS}
    for key in ("attempted", "enqueued", "written", "lastEnqueuedSequence", "lastWrittenSequence"):
        counters[key] = str(len(rows))
    counters["queueHighWater"] = "1"
    control = dict(type="checkpoint" if role == "parent" else "footer", reason="stopped" if role == "parent" else "normal",
        report=dict(configured=True, enabled=True, complete=True, valid=True, finished=role == "worker", throughSequence=str(len(rows)), counters=counters))
    return [header(role), *rows, control]


class Fixture:
    def __init__(self, count=4, delta=1000):
        self.parent = [event("ChannelOpened", [7, 20], 300)]
        self.worker = []
        for source in range(2):
            self.parent.append(event("SourceBound", [500+source, 700, 1, 600+source, 10, 20, 100+source, 900], 500))
        self.parent.append(event("Marker", [8000, 1], 1000))
        for index in range(count):
            source = index % 2
            capture, frame, typed, effect = 10000+index, 20000+index, 30000+index, 40000+index
            at = 2000 + index * 20000
            operation, sequence = index+1, index+1
            # Deliberate same native gen/seq across two sources. Every second
            # frame for one source resets native seq under a NEW private domain.
            domain, native_sequence = 30 + index // 2, 1
            logical_sequence, logical_time = index // 2 + 1, 1_000_000 + index
            self.parent += [event("SourceCapture", [capture, 10, 100+source, 80+source, 8, 90, logical_sequence, 2], at),
                event("NativeEnqueue", [capture, 0, frame, 10, 20, domain, 100+source, 8, native_sequence, 400, logical_time, 1], at+5),
                # Same capture/change fans to a second subscription. It is not
                # the captured ProfileSession's typed input and must not collide.
                event("NativeEnqueue", [capture, 0, frame+100000, 10, 21, domain, 100+source, 8, native_sequence, 400, logical_time, 1], at+6),
                event("NativeEnqueue", [capture, 1, frame+200000, 10, 20, domain, 100+source, 8, native_sequence+1, 401, logical_time, 2], at+7),
                event("SourceResult", [capture, 1, 0], at+8),
                event("NativeDequeue", [frame, 10, 20, domain, 100+source, 8, native_sequence, 0], at+10),
                event("TypedAlias", [500+source, 10, 20, domain, 100+source, 8, native_sequence,
                    typed, 600+source, 9, logical_sequence, 402, 400, logical_time, 0], at+20),
                event("Effect", [effect, 800, 700, 1, index+1, 1], at+30),
                event("EffectRoot", [effect, 600+source, 402, 9, 0, logical_sequence, logical_time], at+31),
                event("DeliveryBound", [effect, 800, 700, 1, operation, 900+source], at+35),
                event("RequestIssued", [7, sequence, operation, 3, 700, 1], at+40),
                event("RequestQueued", [7, sequence, 1], at+45),
                event("RequestDequeued", [7, sequence, 0], at+50),
                event("ParentOutcome", [7, sequence, operation, 5, 1, 1, 0], at+delta+20)]
            self.worker += [event("RequestQueued", [7, sequence, 1], at+60), event("RequestDequeued", [7, sequence, 0], at+70),
                event("SubmitReturned", [7, sequence, operation, 1, 1, 1], at+delta),
                event("WorkerOutcome", [7, sequence, operation, 5, 1, 1], at+delta+10)]
        end = 2000 + (count-1) * 20000 + delta + 100
        self.parent += [event("Marker", [8001, 2], end), event("RequestIssued", [7, count+1, 0, 7, 0, 0], end+10),
            event("ParentOutcome", [7, count+1, 0, 5, 0, 0, 0], end+30), event("ChannelExited", [7, 20, 0, 1, 1], end+40)]
        self.worker.append(event("WorkerOutcome", [7, count+1, 0, 5, 0, 0], end+20))
        self.count = count

    def rows(self):
        return complete(self.parent, "parent"), complete(self.worker, "worker")

    def run(self, mutate=None, **options):
        parent, worker = self.rows()
        external = copy.deepcopy(parent[-1]["report"])
        if mutate:
            mutate(parent, worker)
        with tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parent) as directory:
            for name, rows in (("parent", parent), ("worker", worker)):
                Path(directory, name + ".jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
            return audit.analyze(directory, options.pop("phases", [audit.Phase("idle", 1, 2, self.count)]), parent_report=options.pop("parent_report", external), **options)


class LatencyTests(unittest.TestCase):
    def test_exact_two_source_batched_axes_and_queue_reset_join(self):
        report = Fixture().run()
        self.assertTrue(report["traceValid"])
        self.assertTrue(report["passed"])
        phase = report["phases"][0]
        self.assertEqual((phase["admittedChanges"], phase["selectedKeyChanges"], phase["successfulKeyEdges"]), (8, 4, 4))
        self.assertEqual(phase["latency"]["p95UsCeil"], 1000)
        self.assertFalse(report["physicalCaptureMeasured"])

    def test_nearest_rank_and_strict_thresholds_use_integer_ticks(self):
        self.assertEqual(audit.nearest_rank(list(range(1, 101)), 95), 95)
        self.assertEqual(audit.nearest_rank(list(range(1, 101)), 99), 99)
        self.assertFalse(Fixture(delta=5000).run()["phases"][0]["latency"]["strictP95Below5ms"])
        self.assertFalse(Fixture(delta=10000).run()["phases"][0]["latency"]["strictP99Below10ms"])
        self.assertFalse(Fixture(delta=5000).run()["passed"])
        self.assertTrue(audit.latency([49999], 10_000_000)["strictP95Below5ms"])

    def test_journal_diagnostics_do_not_replace_or_change_latency_endpoints(self):
        fixture = Fixture()
        baseline = fixture.run()
        position = next(index for index, row in enumerate(fixture.parent)
            if row["kind"] == "RequestQueued")
        fixture.parent.insert(position, event("JournalPersist",
            [7, 1, 2040, 2040, 2041, 2042, 2043, 2043, 2044], 2044))
        report = fixture.run()
        self.assertTrue(report["traceValid"])
        self.assertTrue(report["passed"])
        self.assertEqual(report["phases"], baseline["phases"])

    def test_one_tick_cross_process_interval_is_not_a_latency_sample(self):
        for ticks in (0, 1):
            fixture = Fixture()
            for row in fixture.parent + fixture.worker:
                kind, fields = row["kind"], row["fields"]
                first = ((kind in ("SourceCapture", "SourceResult", "NativeEnqueue") and fields[0] == "10000")
                    or (kind == "NativeDequeue" and fields[0] == "20000")
                    or (kind == "TypedAlias" and fields[7] == "30000")
                    or (kind in ("Effect", "EffectRoot", "DeliveryBound") and fields[0] == "40000")
                    or (kind in ("RequestIssued", "RequestQueued", "RequestDequeued", "SubmitReturned", "WorkerOutcome", "ParentOutcome") and fields[1] == "1"))
                if first:
                    row["qpc"] = str(2000 + (ticks if kind in ("SubmitReturned", "WorkerOutcome", "ParentOutcome") else 0))
            report = fixture.run()
            self.assertFalse(report["traceValid"])
            self.assertIsNone(report["phases"][0]["latency"])
            self.assertTrue(any("one tick or less" in error for error in report["phases"][0]["errors"]))

    def test_two_phases_have_independent_expected_edges_and_integer_percentiles(self):
        fixture = Fixture(count=8)
        end = next(row for row in fixture.parent if row["kind"] == "Marker" and row["fields"][1] == "2")
        end["fields"][1] = "4"
        position = next(index for index, row in enumerate(fixture.parent) if row["kind"] == "SourceCapture" and row["fields"][0] == "10004")
        fixture.parent[position:position] = [event("Marker", [8002, 2], 70000), event("Marker", [8003, 3], 75000)]
        report = fixture.run(phases=[audit.Phase("idle", 1, 2, 4), audit.Phase("ui", 3, 4, 4)])
        self.assertTrue(report["passed"])
        self.assertEqual([phase["successfulKeyEdges"] for phase in report["phases"]], [4, 4])
        self.assertEqual([phase["latency"]["p99UsCeil"] for phase in report["phases"]], [1000, 1000])

    def test_missing_or_ambiguous_immutable_root_withholds_percentiles(self):
        for mode in ("missing", "duplicate", "wrong-generation", "wrong-domain", "wrong-run"):
            fixture = Fixture(count=6)
            if mode == "missing":
                fixture.parent = [row for row in fixture.parent if not (row["kind"] == "EffectRoot" and row["fields"][0] == "40000")]
            elif mode == "duplicate":
                fixture.parent.append(copy.deepcopy(next(row for row in fixture.parent if row["kind"] == "EffectRoot")))
            else:
                row = next(row for row in fixture.parent if row["kind"] == ("SourceBound" if mode == "wrong-run" else "TypedAlias"))
                row["fields"][2 if mode == "wrong-run" else 9 if mode == "wrong-generation" else 3] = "777"
            try:
                report = fixture.run()
            except audit.AuditError:
                continue
            self.assertFalse(report["traceValid"], mode)
            self.assertIsNone(report["phases"][0]["latency"], mode)

    def test_missing_transport_or_ack_or_submission_fails_closed(self):
        for kind in ("RequestQueued", "RequestDequeued", "WorkerOutcome", "SubmitReturned"):
            fixture = Fixture()
            fixture.worker = [row for row in fixture.worker if not (row["kind"] == kind and row["fields"][1] == "1")]
            report = fixture.run()
            self.assertFalse(report["traceValid"], kind)
            self.assertGreater(report["phases"][0]["excluded"]["unacked"], 0)
            self.assertIsNone(report["phases"][0]["latency"])

    def test_noop_collision_and_skipped_are_counted_not_success_samples(self):
        for flag, category in ((5, "noop"), (21, "collision"), (13, "skipped")):
            fixture = Fixture()
            for row in fixture.parent + fixture.worker:
                if row["kind"] in ("ParentOutcome", "WorkerOutcome") and row["fields"][1] == "1":
                    row["fields"][3:6] = [str(flag), "0", "0"]
            fixture.worker = [row for row in fixture.worker if not (row["kind"] == "SubmitReturned" and row["fields"][1] == "1")]
            report = fixture.run()
            self.assertTrue(report["traceValid"])
            self.assertFalse(report["passed"])
            self.assertEqual(report["phases"][0]["excluded"][category], 1)
            self.assertEqual(report["phases"][0]["latency"]["samples"], 3)

    def test_partial_failed_submission_is_excluded_and_ack_mismatch_is_invalid(self):
        fixture = Fixture()
        for row in fixture.parent + fixture.worker:
            if row["kind"] in ("ParentOutcome", "WorkerOutcome") and row["fields"][1] == "1":
                row["fields"][3:6] = ["2", "1", "0"]
                if row["kind"] == "ParentOutcome":
                    row["fields"][6] = "2"
            if row["kind"] == "SubmitReturned" and row["fields"][1] == "1":
                row["fields"][5] = "0"
        report = fixture.run()
        self.assertEqual(report["phases"][0]["excluded"]["fault"], 1)
        self.assertEqual(report["phases"][0]["latency"]["samples"], 3)
        self.assertFalse(report["passed"])
        row = next(row for row in fixture.parent if row["kind"] == "ParentOutcome")
        row["fields"][5] = "1"
        self.assertFalse(fixture.run()["traceValid"])

    def test_worker_identity_clock_footer_and_stopped_proof_are_mandatory(self):
        changes = [lambda p,w: w[0].update(parentInstance="999"), lambda p,w: w[0].update(qpcFrequency="10000000"),
            lambda p,w: w.pop(), lambda p,w: p[-1].update(reason="manual"),
            lambda p,w: w[0].update(processPid="21"), lambda p,w: w[-1]["report"]["counters"].update(queueDrops="1"),
            lambda p,w: p[-1]["report"]["counters"].update(written="999")]
        for change in changes:
            with self.assertRaises(audit.AuditError):
                Fixture().run(change)

    def test_stale_valid_footer_cannot_hide_finish_timeout_or_kill(self):
        for code, success, reason in ((87, 0, 1), (0, 1, 2), ((1 << 64)-1, 0, 3)):
            fixture = Fixture()
            exited = next(row for row in fixture.parent if row["kind"] == "ChannelExited")
            exited["fields"][2:] = [str(code), str(success), str(reason)]
            with self.assertRaises(audit.AuditError):
                fixture.run()
        fixture = Fixture()
        fixture.parent = [row for row in fixture.parent if row["kind"] != "ChannelExited"]
        with self.assertRaises(audit.AuditError):
            fixture.run()

    def test_returned_parent_report_is_independent_of_stale_raw_checkpoint(self):
        fixture = Fixture()
        parent, _ = fixture.rows()
        external = copy.deepcopy(parent[-1]["report"])
        external["valid"] = False
        external["counters"]["flushTimeouts"] = "1"
        with self.assertRaises(audit.AuditError):
            fixture.run(parent_report=external)
        with self.assertRaises(audit.AuditError):
            fixture.run(parent_report=None)
        wrapped = dict(ok=True, state=dict(desktop=dict(audit=parent[-1]["report"])))
        self.assertTrue(fixture.run(parent_report=wrapped)["passed"])

    def test_replayed_records_and_noncanonical_numeric_fields_are_refused(self):
        for change in (lambda p,w: p[2].update(sequence="1"), lambda p,w: p[1].update(qpc=100),
            lambda p,w: p[1]["fields"].__setitem__(0, "01"), lambda p,w: p[1]["fields"].__setitem__(0, str(1 << 64))):
            with self.assertRaises(audit.AuditError):
                Fixture().run(change)

    def test_duplicate_in_one_subscription_is_not_hidden_by_fanout_deduplication(self):
        fixture = Fixture()
        frame = copy.deepcopy(next(row for row in fixture.parent if row["kind"] == "NativeEnqueue"))
        frame["fields"][2] = "99999"
        frame["fields"][8] = "999"
        fixture.parent.append(frame)
        with self.assertRaises(audit.AuditError):
            fixture.run()

    def test_marker_duplicates_and_total_absence_cannot_pass(self):
        fixture = Fixture()
        fixture.parent.append(event("Marker", [9000, 1], 1001))
        with self.assertRaises(audit.AuditError):
            fixture.run()
        self.assertTrue(fixture.run(phases=[audit.Phase("idle", 1, 2, 4, begin_id=8000, end_id=8001)])["passed"])
        fixture = Fixture()
        fixture.parent = [row for row in fixture.parent if row["kind"] != "DeliveryBound"]
        with self.assertRaises(audit.AuditError):
            fixture.run()

    def test_missing_selected_key_change_and_worker_queue_fault_cannot_pass(self):
        fixture = Fixture(count=6)
        fixture.parent = [row for row in fixture.parent if not (row["kind"] == "DeliveryBound" and row["fields"][0] == "40000")]
        report = fixture.run()
        self.assertFalse(report["passed"])
        self.assertEqual(report["phases"][0]["excluded"]["unresolved"], 1)
        self.assertIsNone(report["phases"][0]["latency"])
        fixture = Fixture()
        fixture.worker.append(event("QueueFault", [2, 1, 32], 2100))
        report = fixture.run()
        self.assertFalse(report["traceValid"])
        self.assertEqual(report["phases"][0]["queueFaults"], 1)


if __name__ == "__main__":
    unittest.main()
