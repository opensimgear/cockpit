"""Offline checks of workload accounting; never native performance evidence."""
import copy
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('desktop_latency_workload', Path(__file__).resolve().parents[2] / 'scripts/desktop-latency-native.py')
latency = importlib.util.module_from_spec(spec)
spec.loader.exec_module(latency)


class Clock:
    now = 0.0
    def read(self): return self.now
    def wait(self, delay): self.now += max(0, delay)
    def is_set(self): return False
    def set(self): raise AssertionError('Unexpected workload failure')


class Gate:
    def wait(self, timeout): return None


class WorkloadTests(unittest.TestCase):
    def run_workload(self, stall=False):
        clock, results, errors, sends = Clock(), [None, None], [], []
        class Peer: name = 'authored-peer-a'
        def send(_peer, changes):
            sends.append((clock.now, changes))
            if stall and len(sends) == 4: clock.now += .07
            return str(len(sends))
        with patch.object(latency.time, 'perf_counter', clock.read), patch.object(latency, 'send', send):
            latency.workload(Peer(), 0, Gate(), clock, results, errors, seconds=.125)
        self.assertEqual(errors, [])
        return results[0], sends

    def test_only_actual_changed_key_frames_are_counted(self):
        record, sends = self.run_workload()
        actual_keys = [(str(index + 1), change['value']) for index, (_at, changes) in enumerate(sends)
            for change in changes if change['controlId'] == latency.BUTTON]
        self.assertEqual(actual_keys, [('1', True), ('5', False), ('9', True)])
        self.assertEqual(record['keyChanges'], [dict(wireSequence=sequence, down=down) for sequence, down in actual_keys])
        self.assertEqual(record['expectedImmediateKeyChanges'], 3)
        self.assertEqual(record['axisChanges'], len(sends))
        self.assertTrue(all(changes[0]['controlId'] == latency.AXIS for _at, changes in sends))
        self.assertEqual(record['keyHid'], 4)
        self.assertEqual(record['configuredDelayUs'], '0')

    def test_a_stall_is_reported_without_manufacturing_a_catchup_burst(self):
        record, sends = self.run_workload(stall=True)
        self.assertGreater(record['schedulerLateIntervals'], 0)
        self.assertTrue(all(b[0] - a[0] >= 1 / latency.RATE - 1e-10 for a, b in zip(sends, sends[1:])))
        self.assertEqual(record['inputFrames'], len(sends))

    def test_wire_axis_stays_normalized_and_requires_actual_native_integer_contract(self):
        observed = dict(delivery='state', fallback=0, freshnessMs=1500,
            schema=dict(type='integer', min=-32768, max=32767, unit=None))
        boundary = latency.axis_boundary(observed)
        self.assertEqual(boundary['wireRange'], [-1, 1])
        self.assertAlmostEqual(boundary['explicitScaleFactor'], 1 / 32768)
        record, _sends = self.run_workload()
        self.assertTrue(all(-1 <= frame['changes'][0]['value'] <= 1 for frame in record['frames']))
        with self.assertRaises(AssertionError):
            latency.axis_boundary(dict(schema=dict(type='integer', min=-1, max=1)))
        with self.assertRaises(AssertionError):
            latency.axis_boundary(dict(schema=dict(type='number', min=-32768, max=32767)))

    def test_native_marker_ids_must_be_canonical_decimal(self):
        with patch.object(latency.mapping, 'state', return_value={'auditMarker': {'id': '01', 'phase': 1}}):
            with self.assertRaises(AssertionError): latency.marker(None, 1)

    def test_unavailable_or_invalid_audit_cannot_authorize_workload(self):
        report = dict(configured=True, enabled=True, complete=True, valid=False, throughSequence='0', counters={})
        with patch.object(latency.mapping, 'state', return_value={'audit': report}):
            with self.assertRaises(AssertionError): latency.audit_checkpoint(None)

    def test_resource_catalog_must_match_both_applied_source_generations(self):
        class Peer: pass
        checkpoints = iter(({'a': {'generation': 3}, 'b': {'generation': 2}},
            {'a': {'generation': 4}, 'b': {'generation': 2}}))
        owners = [dict(actionId='key-' + label, producer=dict(dependencies=[dict(generation=str(generation))]))
            for label, generation in [('a', 4), ('b', 2)]]
        attempts = []
        def wait(predicate, _message, seconds):
            for _ in range(2):
                value = predicate()
                attempts.append(value)
                if value: return value
            raise AssertionError('Never correlated')
        with patch.object(latency.companion, 'companion_checkpoint', side_effect=lambda *_: next(checkpoints)), \
             patch.object(latency.native, 'desktop_status', return_value={'runtime': {'backend': {'owners': owners}}}), \
             patch.object(latency.mapping, 'wait_for', wait):
            value = latency.coherent_sources(None, [('a', {}), ('b', {})], [Peer(), Peer()])
        self.assertIsNone(attempts[0])
        self.assertEqual(value['a']['generation'], 4)

    def test_receiver_and_native_ownership_must_both_catch_up(self):
        records = [dict(scanCode=30, keyHid=4, actionId='key-a', finalPressed=False,
            keyChanges=[dict(down=True), dict(down=False)])]
        status = dict(enabled=True, runtime=dict(error=None, scopeError=None, state='active', waitingNeutral=[], backend=dict(
            error=None, uncertain=False, inFlight=None, possibleHeld=[], held=[], owners=[dict(actionId='key-a', controls=[])])))
        observations = [dict(injectedMarker=True, message=message, scanCode=30, extended=0) for message in (256, 257)]
        def once(predicate, *_args, **_kwargs): return predicate()
        with patch.object(latency.native, 'desktop_status', return_value=status), patch.object(latency.mapping, 'wait_for', once):
            with patch.object(latency.native, 'receiver', return_value={'observations': observations[:-1]}):
                self.assertIsNone(latency.settle_keys(None, None, records))
            held = copy.deepcopy(status)
            held['runtime']['backend']['owners'][0]['controls'] = [dict(kind='key', key=4)]
            with patch.object(latency.native, 'receiver', return_value={'observations': observations}), \
                 patch.object(latency.native, 'desktop_status', return_value=held):
                self.assertIsNone(latency.settle_keys(None, None, records))
            with patch.object(latency.native, 'receiver', return_value={'observations': observations + observations[:1]}):
                with self.assertRaises(AssertionError): latency.settle_keys(None, None, records)
            with patch.object(latency.native, 'receiver', return_value={'observations': observations}):
                self.assertIsNotNone(latency.settle_keys(None, None, records))

    def test_exact_warmup_prefix_is_excluded_and_must_still_match(self):
        records = [dict(scanCode=30, keyHid=4, actionId='key-a', finalPressed=False,
            keyChanges=[dict(down=True), dict(down=False)])]
        status = dict(enabled=True, runtime=dict(error=None, scopeError=None, state='active', waitingNeutral=[], backend=dict(
            error=None, uncertain=False, inFlight=None, possibleHeld=[], held=[], owners=[dict(actionId='key-a', controls=[])])))
        events = [dict(injectedMarker=True, message=message, scanCode=30, extended=0, atUs=str(index))
            for index, message in enumerate((256, 257, 256, 257))]
        prefix = {'30': events[:2]}
        with patch.object(latency.native, 'desktop_status', return_value=status), \
             patch.object(latency.mapping, 'wait_for', side_effect=lambda predicate, *_args, **_kwargs: predicate()):
            with patch.object(latency.native, 'receiver', return_value={'observations': events}):
                value = latency.settle_keys(None, None, records, receiver_prefix=prefix)
                self.assertEqual(value['keyCounts'], [dict(actionId='key-a', warmupEdgesExcluded=2,
                    expectedChangedEdges=2, observedChangedEdges=2)])
                with self.assertRaises(AssertionError): latency.settle_keys(None, None, records)
                altered = copy.deepcopy(prefix)
                altered['30'][0]['atUs'] = '999'
                with self.assertRaises(AssertionError): latency.settle_keys(None, None, records, receiver_prefix=altered)
            with patch.object(latency.native, 'receiver', return_value={}):
                self.assertIsNone(latency.settle_keys(None, None, records, receiver_prefix=prefix))

    def test_pointer_load_requires_new_marked_receiver_movement_and_both_native_actions(self):
        def outcome(action, operation):
            return dict(delivery=dict(actionId=action, operationId=str(operation), intent=dict(kind='pointer')),
                acknowledgement=dict(ok=True, uncertain=False, skipped=False, attempted=1, confirmed=1))
        moves = dict(observations=[dict(message=512, injectedMarker=True), dict(message=512, injectedMarker=True)])
        status = dict(runtime=dict(outcomes=[outcome('pointer-a', 11), outcome('pointer-b', 12)]))
        value = latency.pointer_load_proof(moves, status, 1, 10)
        self.assertEqual(value['receiverMarkedMouseMoves'], 1)
        self.assertEqual(value['recentNativePointerCounts']['pointer-b']['nativeEventsConfirmed'], 1)
        self.assertIsNone(latency.pointer_load_proof(moves, status, 2, 10))
        self.assertIsNone(latency.pointer_load_proof(moves, status, 1, 11))
        unmarked = dict(observations=[dict(message=512, injectedMarker=False)])
        self.assertIsNone(latency.pointer_load_proof(unmarked, status, 0, 10))
        failed = copy.deepcopy(status)
        failed['runtime']['outcomes'][1]['acknowledgement']['confirmed'] = 0
        with self.assertRaises(AssertionError): latency.pointer_load_proof(moves, failed, 1, 10)

    def test_warmup_settles_down_before_release_and_records_exact_exclusion_prefix(self):
        class Peer:
            def __init__(self, name): self.name = name
        sends, stages = [], []
        events = [dict(injectedMarker=True, message=message, scanCode=scan, extended=0)
            for scan in (30, 48) for message in (256, 257)]
        def send(peer, changes):
            sends.append((peer.name, changes[0]['value']))
            return str(len(sends))
        def settle(_service, _path, records):
            stages.append(copy.deepcopy(records))
            return dict(receiver=dict(observations=events if len(stages) == 2 else events[:1]))
        with patch.object(latency, 'send', send), patch.object(latency, 'settle_keys', settle), \
             patch.object(latency.native, 'desktop_status', return_value=dict(runtime=dict(outcomes=[dict(delivery=dict(operationId='9'))]))):
            value = latency.warmup_keys(None, None, [Peer('a'), Peer('b')])
        self.assertEqual(sends, [('a', True), ('b', True), ('a', False), ('b', False)])
        self.assertTrue(all(record['finalPressed'] for record in stages[0]))
        self.assertTrue(all(not record['finalPressed'] for record in stages[1]))
        self.assertEqual([len(value['receiverKeyPrefix'][str(scan)]) for scan in (30, 48)], [2, 2])
        self.assertEqual(value['pointerBaseline'], dict(observation_start=4, operation_baseline='9'))


if __name__ == '__main__':
    unittest.main()
