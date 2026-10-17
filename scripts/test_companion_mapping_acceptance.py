import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import unittest
from unittest.mock import patch
import threading

spec = importlib.util.spec_from_file_location("companion_mapping_acceptance", Path(__file__).with_name("companion-mapping-acceptance.py"))
acceptance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(acceptance)


class Transport:
    def __init__(self, data):
        self.data = bytearray(data)
        self.sent = []

    def recv(self, size):
        result = bytes(self.data[:size])
        del self.data[:size]
        return result

    def sendall(self, data):
        self.sent.append(data)


def server_frame(value):
    data = json.dumps(value, separators=(",", ":")).encode()
    return (bytes([0x81, len(data)]) if len(data) < 126 else bytes([0x81, 126]) + struct.pack("!H", len(data))) + data


def socket_for(data):
    socket = acceptance.Socket.__new__(acceptance.Socket)
    socket.socket = Transport(data)
    socket.buffer = bytearray()
    return socket


class AcceptanceTests(unittest.TestCase):
    def test_open_does_not_witness_readiness_until_the_neutral_baseline_is_acknowledged(self):
        fixture = ReapplyFixture()
        peer = fixture.peers[0]
        opened = dict(peer.opened, sessionId='acknowledged-new-session')
        calls = []
        class Socket:
            def request(self, value):
                calls.append(value)
                if value['type'] == 'open':
                    return dict(ok=True, **opened)
                self_test.assertIsNone(peer.baseline_session)
                return dict(ok=True)
        self_test = self
        peer.socket = Socket()
        peer.open(peer.name, peer.publication)
        self.assertEqual(peer.ready_session()['sessionId'], opened['sessionId'])
        self.assertEqual([call['type'] for call in calls], ['open', 'baseline'])
        controls = json.loads(peer.publication['canonicalDocument'])['ui']['controls']
        self.assertEqual(calls[1]['values'], {control['id']: False if control['kind'] == 'button' else control.get('fallback', -1) for control in controls})
        self.assertEqual(calls[1]['sequence'], '0')

    def test_old_immediate_apply_rejects_the_stop_reconnect_gap(self):
        fixture = ReapplyFixture()
        fixture.service.profiles('profile_stop')
        with self.assertRaisesRegex(AssertionError, 'Connect the exact paired companion'):
            acceptance.mapping.apply(fixture.service, fixture.document)
        self.assertEqual(fixture.service.successful_applies, 0)

    def test_reapply_waits_for_new_acknowledged_sessions_and_exact_native_routes(self):
        fixture = ReapplyFixture()
        observed = []
        def controlled_wait(predicate, message):
            fixture.service.events.append('wait')
            self.assertEqual(fixture.service.events, ['profile_stop', 'profile_runtime_inspect', 'wait'])
            for phase in range(7):
                fixture.stage(phase)
                observed.append(bool(predicate()))
                if observed[-1]:
                    return True
            raise AssertionError(message)
        with patch.object(acceptance, 'wait_for', side_effect=controlled_wait):
            acceptance.stop_and_wait_native_companions(fixture.service, fixture.configurations, fixture.peers, dict(session='old-native-owner'))
            owner = acceptance.mapping.apply(fixture.service, fixture.document)
        self.assertEqual(observed, [False] * 6 + [True])
        self.assertEqual(fixture.service.successful_applies, 1)
        self.assertEqual(owner['profileId'], fixture.document['id'])
        self.assertEqual(fixture.service.events, ['profile_stop', 'profile_runtime_inspect', 'wait', 'profile_apply'])

    def test_stop_owner_fence_failure_prevents_readiness_wait_and_apply(self):
        fixture = ReapplyFixture()
        fixture.service.old_owner_still_inspectable = True
        with patch.object(acceptance, 'wait_for') as readiness:
            with self.assertRaises(AssertionError):
                acceptance.stop_and_wait_native_companions(fixture.service, fixture.configurations, fixture.peers, dict(session='old-native-owner'))
            readiness.assert_not_called()
        self.assertEqual(fixture.service.events, ['profile_stop', 'profile_runtime_inspect'])
        self.assertEqual(fixture.service.apply_attempts, 0)

    def test_reapply_preserves_peer_failure_and_never_attempts_apply(self):
        fixture = ReapplyFixture()
        previous = acceptance.companion_checkpoint(fixture.service, fixture.configurations, fixture.peers)
        fixture.service.profiles('profile_stop')
        fixture.peers[1].failure = RuntimeError('exact WSS baseline failure')
        with patch.object(acceptance, 'wait_for', side_effect=lambda predicate, message: predicate()):
            with self.assertRaisesRegex(AssertionError, 'exact WSS baseline failure'):
                acceptance.wait_native_companions(fixture.service, fixture.configurations, fixture.peers, previous)
        self.assertEqual(fixture.service.apply_attempts, 0)

    def test_reapply_rejects_foreign_identity_even_while_another_peer_is_not_ready(self):
        fixture = ReapplyFixture()
        previous = acceptance.companion_checkpoint(fixture.service, fixture.configurations, fixture.peers)
        fixture.service.profiles('profile_stop')
        fixture.peers[1].opened['controllerInstanceId'] = 'foreign-instance'
        with patch.object(acceptance, 'wait_for', side_effect=lambda predicate, message: predicate()):
            with self.assertRaisesRegex(AssertionError, 'foreign-instance'):
                acceptance.wait_native_companions(fixture.service, fixture.configurations, fixture.peers, previous)
        self.assertEqual(fixture.service.apply_attempts, 0)

    def test_reapply_missing_route_times_out_without_apply(self):
        fixture = ReapplyFixture()
        previous = acceptance.companion_checkpoint(fixture.service, fixture.configurations, fixture.peers)
        fixture.service.profiles('profile_stop')
        fixture.stage(2)
        def deadline(predicate, message):
            self.assertFalse(predicate())
            raise AssertionError(message)
        with patch.object(acceptance, 'wait_for', side_effect=deadline):
            with self.assertRaisesRegex(AssertionError, 'native companion'):
                acceptance.wait_native_companions(fixture.service, fixture.configurations, fixture.peers, previous)
        self.assertEqual(fixture.service.apply_attempts, 0)

    def test_reapply_rejects_changed_native_identity_or_unverified_route_without_apply(self):
        for change in ['identity', 'duplicate', 'transport', 'generation']:
            with self.subTest(change=change):
                fixture = ReapplyFixture()
                previous = acceptance.companion_checkpoint(fixture.service, fixture.configurations, fixture.peers)
                fixture.service.profiles('profile_stop')
                fixture.stage(6)
                resource = fixture.service.details[1]['resource']
                if change == 'identity':
                    resource['id'] = 'foreign-native-resource'
                elif change == 'duplicate':
                    resource['connections'].append(resource['connections'][0])
                elif change == 'transport':
                    resource['connections'][0]['route']['transport'] = 'synthetic'
                else:
                    resource['connections'][0]['route']['providerGeneration'] = '0'
                with patch.object(acceptance, 'wait_for', side_effect=lambda predicate, message: predicate()):
                    with self.assertRaises(AssertionError):
                        acceptance.wait_native_companions(fixture.service, fixture.configurations, fixture.peers, previous)
                self.assertEqual(fixture.service.apply_attempts, 0)

    def test_client_frames_are_masked_and_bounded_for_both_length_encodings(self):
        for length in [1, 125, 126, acceptance.FRAME_BYTES]:
            payload = b"a" * length
            encoded = acceptance.encoded_frame(payload)
            self.assertEqual(encoded[0], 0x81)
            self.assertTrue(encoded[1] & 0x80)
            offset = 2 if length < 126 else 4
            if offset == 4:
                self.assertEqual(struct.unpack("!H", encoded[2:4])[0], length)
            mask = encoded[offset:offset + 4]
            self.assertEqual(bytes(value ^ mask[index % 4] for index, value in enumerate(encoded[offset + 4:])), payload)
        with self.assertRaises(AssertionError):
            acceptance.encoded_frame(b"a" * (acceptance.FRAME_BYTES + 1))

    def test_server_frames_fail_before_accepting_fragmentation_masking_or_large_payloads(self):
        self.assertEqual(socket_for(server_frame({"ok": True})).frame(), {"ok": True})
        for header in [b"\x01\x00", b"\x81\x80", b"\x81\x7f" + struct.pack("!Q", acceptance.FRAME_BYTES + 1)]:
            with self.assertRaises(AssertionError):
                socket_for(header).frame()
        with self.assertRaises(RuntimeError):
            socket_for(b"").frame()

    def test_publication_transfer_preserves_bytes_and_rejects_corruption_or_reordering(self):
        payload = b'{"canonicalDocument":"exact bytes"}'
        start = {"type": "transfer-start", "id": "transfer-a", "bytes": len(payload), "sha256": "sha256:" + hashlib.sha256(payload).hexdigest()}
        chunk = {"type": "transfer-chunk", "id": "transfer-a", "index": 0, "base64": base64.b64encode(payload).decode()}
        end = {"type": "transfer-end", "id": "transfer-a"}
        self.assertEqual(socket_for(b"".join(server_frame(item) for item in [start, chunk, end])).request({}), json.loads(payload))
        for bad in [{**chunk, "index": 1}, {**chunk, "base64": base64.b64encode(b"x" * len(payload)).decode()}, {**chunk, "id": "other"}]:
            with self.assertRaises(AssertionError):
                socket_for(b"".join(server_frame(item) for item in [start, bad, end])).request({})

    def test_profile_uses_only_native_observed_ports_and_exact_contract_hash_pins(self):
        publication=json.loads((acceptance.ROOT/'crates/companion-service/tests/fixtures/c1-flight-publication.json').read_text())
        service=FixtureService([('peer-a',publication),('peer-b',publication)])
        document,assignments,models=acceptance.companion_profile(service,[('peer-a',publication),('peer-b',publication)])
        self.assertEqual(document['schemaVersion'],2)
        self.assertEqual(len(document['resources']),2)
        self.assertEqual(len({row['sourceId'] for row in assignments}),2)
        self.assertEqual(len(document['bindings']),6)
        for resource,assignment in zip(document['resources'],assignments):
            self.assertEqual(len(resource['ports']),3)
            self.assertEqual({pin['expectedContractHash'] for pin in assignment['ports']},{'sha256:'+'a'*64})
            self.assertEqual(resource['kind'],'companion')
        self.assertNotIn('peer-a',json.dumps(document))
        self.assertEqual(acceptance.expected(publication),{'button':True,'axis':24575,'hat':7})
        self.assertEqual(acceptance.axis_value(-.5),-16384)
        self.assertEqual({row['publicationHash'] for row in document['companionAssignments']},{publication['publicationHash']})


class FixtureService:
    """Only a DTO fixture; actual service scenarios still require native replies."""
    def __init__(self,configurations):
        self.details=[]
        for index,(instance,publication) in enumerate(configurations):
            resource=dict(id=f'11111111-1111-4111-8111-{index:012d}',resourceType='companion')
            ui=json.loads(publication['canonicalDocument'])['ui'];ports=[]
            for control in ui['controls']:
                schema=dict(type='boolean') if control['kind']=='button' else dict(type='integer',min=-1 if control['kind']=='hat' else -32768 if control.get('axisMode')=='bipolar' else 0,max=7 if control['kind']=='hat' else 32767,unit=None)
                fallback=False if control['kind']=='button' else -1 if control['kind']=='hat' else acceptance.axis_value(control.get('fallback',0))
                ports.append(dict(descriptor=dict(id=control['id'],name=control['id'],direction='output',contract=dict(delivery='state',schema=schema,freshnessMs=1000,fallback=fallback)),contractHash='sha256:'+'a'*64,current=True))
            for feedback in ui['feedbackWidgets']:
                schema=dict(type='boolean') if feedback['kind']=='indicator' else dict(type='text',maxBytes=256,encoding='utf8') if feedback['kind']=='text' else dict(type='number',min=0,max=100,unit='%')
                id=f"feedback:{feedback['channelId']}:{feedback['id']}"
                ports.append(dict(descriptor=dict(id=id,name=id,direction='input',contract=dict(delivery='state',schema=schema,freshnessMs=1000,fallback=None)),contractHash='sha256:'+'a'*64,current=True))
            self.details.append(dict(resource=resource,evidence=[dict(kind='companion-instance',value=instance)],ports=ports,total=len(ports)))

    def request(self,command,**fields):
        if command=='profile_resources_list':value=dict(offset=0,total=len(self.details),resources=[row['resource'] for row in self.details])
        elif command=='profile_resources_inspect':value=next(row for row in self.details if row['resource']['id']==fields['resourceId'])
        else:raise AssertionError(command)
        return dict(ok=True,state=dict(profileResources=value))


class ReapplyFixture:
    """Controlled peer acknowledgements/inventory; no socket, thread or service."""
    def __init__(self):
        publication = json.loads((acceptance.ROOT/'crates/companion-service/tests/fixtures/c1-flight-publication.json').read_text())
        self.configurations = [('peer-a', publication), ('peer-b', publication)]
        self.peers = []
        for instance, panel in self.configurations:
            peer = acceptance.Peer.__new__(acceptance.Peer)
            peer.name = peer.instance = instance
            peer.publication = panel
            peer.lock = threading.Lock()
            peer.failure = None
            peer.opened = dict(clientId=f'client-{instance}',sessionId=f'old-{instance}',generation='1',controllerInstanceId=instance,publicationHash=panel['publicationHash'])
            peer.baseline_session = peer.opened['sessionId']
            self.peers.append(peer)
        self.service = ReapplyService(self)
        self.document = dict(id='11111111-1111-4111-8111-111111111111',revision=1)
        self.phase = -1
        self.stage(-1)

    def stage(self, phase):
        self.phase = phase
        for index, (peer, detail) in enumerate(zip(self.peers, self.service.details)):
            peer.opened['sessionId'] = f'{"old" if phase <= 0 else "new"}-{peer.name}'
            peer.baseline_session = None if phase == 1 else peer.opened['sessionId']
            # Stages: no fresh session; no baseline acknowledgement; no route;
            # unbaselined route; stale generation; only one new route; all ready.
            generation = 1 if phase in (-1, 4) or phase == 5 and index == 1 else 2
            route = dict(id=f'companion:{peer.name}',transport='network',providerGeneration=str(generation),baselineReady=phase != 3,writable=phase != 3)
            detail['resource']['connections'] = [] if 0 <= phase <= 2 else [dict(route=route)]
            detail['resource']['attributes'] = dict(publicationHash=peer.publication['publicationHash'])


class ReapplyService(FixtureService):
    def __init__(self, fixture):
        super().__init__(fixture.configurations)
        self.fixture = fixture
        self.apply_attempts = self.successful_applies = 0
        self.events = []
        self.old_owner_still_inspectable = False

    def request(self, command, **fields):
        if command == 'profile_runtime_inspect':
            self.events.append(command)
            return dict(ok=self.old_owner_still_inspectable)
        return super().request(command, **fields)

    def profiles(self, command, **fields):
        self.events.append(command)
        if command == 'profile_stop':
            self.fixture.stage(0)
            return {}
        if command != 'profile_apply':
            raise AssertionError(command)
        self.apply_attempts += 1
        assert self.fixture.phase == 6, 'Connect the exact paired companion through one verified native route before applying its panel.'
        self.successful_applies += 1
        return dict(profileSession=dict(state='running',owner=dict(profileId=fields['id'],profileRevision=fields['expectedRevision'])))


if __name__=='__main__':unittest.main()
