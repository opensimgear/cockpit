import copy
import importlib.util
import hashlib
import json
from pathlib import Path
import struct
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('companion_feedback_acceptance', Path(__file__).with_name('companion-feedback-acceptance.py'))
acceptance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(acceptance)
PUBLICATION = json.loads((acceptance.ROOT / 'crates/companion-service/tests/fixtures/c1-flight-publication.json').read_text())
IDENTITY = dict(clientId='client', sessionId='session', generation='1', controllerInstanceId='instance', publicationHash=PUBLICATION['publicationHash'])


def sample(output, reading=None, status='fresh'):
    value = dict(channelId=f'flight-channel-{output}', outputId=f'flight-output-{output}', status=status, provenance='game-confirmed', sourceKind='simulator', sourceAgeMs=0, unit='%' if output == 2 else 'state')
    if output == 2:
        value['range'] = dict(min=0, max=100)
    if status == 'fresh':
        value['value'] = reading
    return value


def frame(mode='snapshot', sequence='1', generation='1', values=None):
    result = dict(version=1, type='feedback', **IDENTITY, sequence=sequence, bindingGeneration=generation, mode=mode, values=values or [sample(0, True), sample(1, 'Ready'), sample(2, 42)])
    if mode == 'snapshot':
        result['bindings'] = [dict(channelId=f'flight-channel-{i}', outputId=f'flight-output-{i}') for i in range(3)]
    return result


class FeedbackAcceptanceTests(unittest.TestCase):
    def test_decimal_counters_are_lossless_and_strict(self):
        self.assertEqual(acceptance.counter(str(2**64 - 1)), 2**64 - 1)
        self.assertEqual(acceptance.counter('0'), 0)
        for bad in ['', '01', '-1', str(2**64), 1, True, '\u0031.0', '\uff11']:
            with self.assertRaises(AssertionError):
                acceptance.counter(bad)

    def test_snapshots_and_deltas_keep_per_source_freshness_and_cannot_refresh_omitted_values(self):
        state = acceptance.FeedbackState(IDENTITY, PUBLICATION)
        with patch.object(acceptance.time, 'monotonic', return_value=10):
            state.apply(frame())
        with patch.object(acceptance.time, 'monotonic', return_value=12):
            state.apply(frame('delta', '2', values=[sample(0, False)]))
            values = state.snapshot()
            self.assertEqual(values['flight-channel-0']['status'], 'fresh')
            self.assertEqual(values['flight-channel-2']['status'], 'stale')
            self.assertEqual(values['flight-channel-2']['sourceAgeMs'], 2000)
        duplicate = frame('delta', '2', values=[sample(0, True)])
        with patch.object(acceptance.time, 'monotonic', return_value=13):
            state.apply(duplicate)
            self.assertFalse(state.snapshot()['flight-channel-0']['value'])
            self.assertEqual(state.snapshot()['flight-channel-0']['status'], 'stale')
        self.assertEqual(state.modes, {'snapshot', 'delta'})
        self.assertLessEqual(len(state.values), 64)

    def test_wrong_identity_revision_or_output_never_partially_changes_received_state(self):
        state = acceptance.FeedbackState(IDENTITY, PUBLICATION)
        state.apply(frame())
        before = copy.deepcopy(state.values)
        for key in ['clientId', 'sessionId', 'generation', 'controllerInstanceId', 'publicationHash']:
            bad = frame('delta', '2')
            bad[key] = 'old'
            with self.assertRaises(AssertionError):
                state.apply(bad)
            self.assertEqual(state.values, before)
        for change in [dict(outputId='wrong'), dict(status='unknown', value=True), dict(unit='percent'), dict(value=True), dict(provenance='game-confirmed', sourceKind='host')]:
            bad = frame('delta', '2', values=[sample(0, False), {**sample(2, 50), **change}])
            with self.assertRaises((AssertionError, KeyError)):
                state.apply(bad)
            self.assertEqual(state.values, before)
            self.assertEqual(state.sequence, 1)

    def test_complete_snapshots_and_current_generations_are_required(self):
        state = acceptance.FeedbackState(IDENTITY, PUBLICATION)
        for bad in [frame('delta'), {**frame(), 'values': [sample(0, True)]}, {**frame(), 'extra': True}]:
            with self.assertRaises(AssertionError):
                state.apply(bad)
        state.apply(frame())
        for bad in [frame('delta', '0'), frame('delta', '2', '0'), frame('delta', '2', '2')]:
            with self.assertRaises(AssertionError):
                state.apply(bad)
        oversized = frame()
        oversized['values'][1]['value'] = 'x' * acceptance.FEEDBACK_BYTES
        with self.assertRaises(AssertionError):
            state.apply(oversized)
        state.apply(frame('snapshot', '2', '2', [sample(i, status='unknown') for i in range(3)]))
        self.assertTrue(all('value' not in value for value in state.snapshot().values()))

    def test_dcs_fixture_uses_complete_bounded_export_words_without_any_command(self):
        packet = acceptance.dcs_packet(257, volume=32768, frequency='251.000', caution=True)
        self.assertEqual(packet[:4], b'\x55' * 4)
        blocks, offset = {}, 4
        while offset < len(packet):
            address, count = struct.unpack_from('<HH', packet, offset)
            offset += 4
            blocks[address] = packet[offset:offset + count]
            self.assertEqual(len(blocks[address]), count)
            self.assertEqual(count % 2, 0)
            offset += count
        self.assertEqual(blocks[64], b'A-10C' + b' ' * 19)
        self.assertEqual(blocks[4704], b'251.000\0')
        self.assertEqual(struct.unpack('<H', blocks[4720])[0], 32768)
        self.assertEqual(struct.unpack('<H', blocks[65534])[0], 1)
        with self.assertRaises(AssertionError):
            acceptance.dcs_packet(1, frequency='not-seven')

    def test_maximum_publication_keeps_all_input_ids_and_exact_feedback_references(self):
        before = copy.deepcopy(PUBLICATION)
        bounded = acceptance.bounded_text_publication(PUBLICATION)
        document = json.loads(bounded['canonicalDocument'])
        original = json.loads(PUBLICATION['canonicalDocument'])
        self.assertEqual(PUBLICATION, before)
        self.assertEqual(document['ui']['controls'], original['ui']['controls'])
        self.assertEqual(document['targets'], original['targets'])
        self.assertEqual(document['bindings'], original['bindings'])
        self.assertEqual(bounded['publicationHash'], 'sha256:' + hashlib.sha256(bounded['canonicalDocument'].encode()).hexdigest())
        outputs = document['ui']['feedbackWidgets']
        self.assertEqual(len(outputs), 64)
        self.assertEqual(len({output['channelId'] for output in outputs}), 64)
        self.assertEqual({item['refId'] for page in document['ui']['pages'] for item in page['items'] if item['kind'] == 'feedback'}, {output['id'] for output in outputs})
        self.assertEqual(bounded, acceptance.bounded_text_publication(PUBLICATION))

    def test_applied_feedback_profile_has_exact_ports_explicit_constant_nodes_and_no_provider_config(self):
        path=Path(__file__).with_name('test_companion_mapping_acceptance.py')
        spec=importlib.util.spec_from_file_location('profile_acceptance_fixtures',path)
        support=importlib.util.module_from_spec(spec);spec.loader.exec_module(support)
        service=support.FixtureService([('peer-a',PUBLICATION),('peer-b',PUBLICATION)])
        document,assignments,_=acceptance.feedback_profile(service,[('peer-a',PUBLICATION),('peer-b',PUBLICATION)])
        self.assertEqual(document['schemaVersion'],2)
        self.assertEqual(len(document['resources']),2)
        self.assertTrue(all(len(row['ports'])==8 for row in document['resources']))
        self.assertTrue(all(node['operation']['kind'] in ['identity','constant'] for node in document['processingNodes']))
        self.assertEqual(document['virtualOutputs'],[])
        for forbidden in ['integrationConfig','provider','baselineToken','ownershipToken','connection']:
            self.assertNotIn(forbidden,json.dumps(document))
        bounded=acceptance.bounded_text_publication(PUBLICATION)
        service=support.FixtureService([('peer-a',bounded)])
        large,local,_=acceptance.feedback_profile(service,[('peer-a',bounded)],large=True)
        self.assertEqual(len(large['resources'][0]['ports']),64)
        self.assertEqual(len(large['bindings']),64)
        self.assertEqual(len(local[0]['ports']),64)
        self.assertEqual(len(large['processingNodes']),1)
        self.assertEqual(len(large['processingNodes'][0]['operation']['value']),256)
        self.assertTrue(all(pin['expectedContractHash']=='sha256:'+'a'*64 for pin in local[0]['ports']))


if __name__=='__main__':unittest.main()
