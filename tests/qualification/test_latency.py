"""Authored evidence verifies the analyzer, never physical performance."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('latency', Path(__file__).resolve().parents[2] / 'scripts/mapping-latency.py')
latency = importlib.util.module_from_spec(spec)
spec.loader.exec_module(latency)


def fixture():
    return dict(schema=1, backend='vjoy', clock='host-monotonic-us', evidenceKind='authored',
                machine=dict(os='fixture', cpu='fixture', appCommit='fixture', driverVersion='fixture', game='none'),
                devices=[dict(source='stick', model='fixture', firmware='fixture', connection='fixture')],
                durationUs=1_000_000,
                diagnostics=dict(queueCapacity=512, queueHighWater=4, coalescedAxisReports=2,
                                 queueGaps=0, missedTransitions=0, feederFaults=0, submitFailures=0),
                samples=[dict(source='stick', kind='builtin', control='axis', sequence=i,
                              capturedUs=i * 10_000, submittedUs=i * 10_000 + 2000,
                              configuredDelayUs=0, successfulSubmit=True) for i in range(100)])


class EvidenceTests(unittest.TestCase):
    def test_exact_nearest_rank_boundaries_do_not_round_into_a_pass(self):
        data = fixture()
        result = latency.summarize(data)
        self.assertEqual(result['latency']['builtin']['p99Us'], 2000)
        self.assertTrue(result['latencyTargetsMet'])
        self.assertFalse(result['qualification'])
        self.assertEqual(result['evidenceKind'], 'authored')
        for entry in data['samples'][-6:]:
            entry['submittedUs'] = entry['capturedUs'] + 5000
        self.assertFalse(latency.summarize(data)['latencyTargetsMet'])
        data = fixture()
        for entry in data['samples'][-2:]:
            entry['submittedUs'] = entry['capturedUs'] + 10000
        self.assertFalse(latency.summarize(data)['latencyTargetsMet'])

    def test_scripts_delays_and_failures_are_reported_separately(self):
        data = fixture()
        data['samples'][1]['kind'] = 'script'
        data['samples'][2]['configuredDelayUs'] = 1000
        data['diagnostics']['missedTransitions'] = 1
        result = latency.summarize(data)
        self.assertEqual(result['latency']['builtin']['count'], 98)
        self.assertEqual(result['latency']['script']['count'], 1)
        self.assertEqual(result['excludedConfiguredDelaySamples'], 1)
        self.assertEqual(result['submittedAxisReportsPerSecond'], 100)
        self.assertTrue(result['latencyTargetsMet'])
        self.assertFalse(result['failureFree'])
        self.assertFalse(result['qualification'])

    def test_malformed_provenance_order_timestamps_and_queue_are_rejected(self):
        changes = [lambda d: d.update(clock='wall-clock'),
                   lambda d: d.update(evidenceKind='qualified'),
                   lambda d: d.update(backend='recording'),
                   lambda d: d.update(durationUs=0),
                   lambda d: d['machine'].pop('driverVersion'),
                   lambda d: d['devices'].append(copy.deepcopy(d['devices'][0])),
                   lambda d: d['samples'][1].update(sequence=0),
                   lambda d: d['samples'][1].update(capturedUs=900_000),
                   lambda d: d['samples'][1].update(submittedUs=1_000_001),
                   lambda d: d['samples'][1].update(source='unknown'),
                   lambda d: d['samples'][1].update(successfulSubmit=False),
                   lambda d: d['samples'][1].update(configuredDelayUs=-1),
                   lambda d: d['diagnostics'].update(queueHighWater=513),
                   lambda d: d['diagnostics'].update(queueGaps=True),
                   lambda d: d['diagnostics'].pop('submitFailures')]
        for change in changes:
            data = fixture()
            change(data)
            with self.assertRaises(ValueError):
                latency.summarize(data)

    def test_missing_builtin_samples_cannot_pass_latency_targets(self):
        data = fixture()
        for entry in data['samples']:
            entry['kind'] = 'script'
        result = latency.summarize(data)
        self.assertIsNone(result['latency']['builtin'])
        self.assertFalse(result['latencyTargetsMet'])


if __name__ == '__main__':
    unittest.main()
