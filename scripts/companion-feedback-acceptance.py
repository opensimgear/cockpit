#!/usr/bin/env python3
"""Independent private Gateway -> native integration -> real WSS feedback acceptance.

Only source-only integrations and recording mappings are started. Simulated DCS
UDP exports exercise the supported decoder; they are not a real game/mobile or
virtual-controller qualification. No device commands, virtual reports or layouts.
"""
import argparse
import copy
import importlib.util
import hashlib
import json
import math
from pathlib import Path
import socket
import struct
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('companion_mapping_acceptance', ROOT / 'scripts/companion-mapping-acceptance.py')
c4 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c4)
FEEDBACK_BYTES = 128 * 1024


def counter(value):
    assert isinstance(value, str) and value.isascii() and value.isdigit() and (value == '0' or not value.startswith('0')) and len(value) <= 20, 'Invalid feedback counter'
    result = int(value)
    assert result <= 2**64 - 1, 'Feedback counter overflow'
    return result


def utf16(value):
    return len(value.encode('utf-16-le')) // 2


class FeedbackState:
    """Independent strict oracle and bounded materialized state, not native code reuse."""
    def __init__(self, opened, publication):
        self.identity = {key: opened[key] for key in ['clientId', 'sessionId', 'generation', 'controllerInstanceId', 'publicationHash']}
        document = json.loads(publication['canonicalDocument'])
        self.outputs = {row['id']: row for row in document['ui']['feedbackWidgets']}
        self.configs = {ref: widget.get('config', {}) for widget in document['presentation']['widgets'] for ref in widget['feedbackIds']}
        self.bindings, self.values = {}, {}
        self.sequence = self.generation = -1
        self.modes = set()
        self.frames = 0

    def sample(self, value, bindings):
        required = {'channelId', 'outputId', 'status', 'provenance', 'sourceKind', 'sourceAgeMs', 'unit'}
        assert isinstance(value, dict) and required <= value.keys() and value.keys() <= required | {'value', 'range'}
        channel, output = value['channelId'], value['outputId']
        assert bindings.get(channel) == output and self.outputs[output]['channelId'] == channel, 'Wrong feedback output/channel'
        assert value['status'] in ['fresh', 'unknown', 'stale', 'disconnected']
        assert value['provenance'] in ['requested', 'local', 'host-accepted', 'host-submitted', 'game-confirmed']
        assert value['sourceKind'] in ['host', 'simulator', 'script']
        assert value['provenance'] != 'game-confirmed' or value['sourceKind'] == 'simulator'
        age = value['sourceAgeMs']
        assert type(age) is int and 0 <= age <= 86_400_000
        assert isinstance(value['unit'], str) and value['unit'].strip() and utf16(value['unit']) <= 120 and all(ord(c) > 31 for c in value['unit'])
        if value['status'] == 'fresh':
            assert 'value' in value and age < 1000
        if value['status'] in ['unknown', 'disconnected']:
            assert 'value' not in value
        kind = self.outputs[output]['kind']
        if kind == 'gauge':
            config = self.configs[output]
            assert value['unit'] == (config['unit'] if config['unit'].strip() else 'ratio') and value.get('range') == {'min': config['min'], 'max': config['max']}
        else:
            assert 'range' not in value
        if 'value' in value:
            reading = value['value']
            if kind == 'indicator':
                assert type(reading) is bool
            elif kind == 'text':
                assert isinstance(reading, str) and utf16(reading) <= 256 and all(ord(c) > 31 for c in reading)
            else:
                assert type(reading) in [int, float] and math.isfinite(reading) and value['range']['min'] <= reading <= value['range']['max']

    def apply(self, frame):
        required = {'version', 'type', 'clientId', 'sessionId', 'generation', 'controllerInstanceId', 'publicationHash', 'sequence', 'bindingGeneration', 'mode', 'values'}
        assert isinstance(frame, dict) and required <= frame.keys() and frame.keys() <= required | {'bindings'}
        assert len(json.dumps(frame, ensure_ascii=False).encode()) <= FEEDBACK_BYTES
        assert frame['version'] == 1 and frame['type'] == 'feedback'
        assert all(frame[key] == expected for key, expected in self.identity.items()), 'Stale feedback identity/revision'
        sequence, generation = counter(frame['sequence']), counter(frame['bindingGeneration'])
        assert generation >= self.generation and sequence >= self.sequence, 'Stale feedback generation/sequence'
        mode = frame['mode']
        assert mode in ['snapshot', 'delta']
        if mode == 'snapshot':
            bindings = {}
            assert isinstance(frame.get('bindings'), list) and len(frame['bindings']) <= 64
            for binding in frame['bindings']:
                assert set(binding) == {'channelId', 'outputId'}
                output = self.outputs[binding['outputId']]
                assert output['channelId'] == binding['channelId'] and binding['channelId'] not in bindings
                bindings[binding['channelId']] = binding['outputId']
        else:
            assert 'bindings' not in frame and self.sequence >= 0 and generation == self.generation, 'Delta without matching snapshot'
            bindings = self.bindings
        assert isinstance(frame['values'], list) and len(frame['values']) <= 64
        changes = {}
        now = time.monotonic()
        for value in frame['values']:
            self.sample(value, bindings)
            assert value['channelId'] not in changes
            changes[value['channelId']] = (copy.deepcopy(value), now)
        if mode == 'snapshot':
            assert set(changes) == set(bindings), 'Incomplete feedback snapshot'
        if sequence == self.sequence and generation == self.generation:
            return
        values = {} if mode == 'snapshot' else dict(self.values)
        values.update(changes)
        self.bindings, self.values = bindings, values
        self.sequence, self.generation = sequence, generation
        self.modes.add(mode)
        self.frames += 1

    def snapshot(self):
        now, values = time.monotonic(), {}
        for channel, (value, received) in self.values.items():
            sample = copy.deepcopy(value)
            sample['sourceAgeMs'] = min(86_400_000, sample['sourceAgeMs'] + int((now - received) * 1000))
            if sample['status'] == 'fresh' and sample['sourceAgeMs'] >= 1000:
                sample['status'] = 'stale'
            values[channel] = sample
        return values


class FeedbackPeer(c4.Peer):
    def __init__(self, service, name):
        self.feedback = None
        self.resync_sequence = 0
        self.modes = set()
        self.max_feedback_bytes = 0
        super().__init__(service, name)

    def consume(self, result):
        assert result['ok'], result
        if self.feedback is not None and 'feedback' in result:
            self.max_feedback_bytes=max(self.max_feedback_bytes,len(json.dumps(result['feedback'],ensure_ascii=False).encode()))
            self.feedback.apply(result['feedback'])
            self.modes.update(self.feedback.modes)

    def renew(self):
        while not self.stopped.wait(0.15):
            if self.paused.is_set():
                continue
            with self.lock:
                if self.opened is None:
                    continue
                try:
                    self.heartbeat += 1
                    self.consume(self.socket.request(self.message('heartbeat', sequence=str(self.heartbeat))))
                except Exception as error:
                    self.failure = error
                    return

    def open(self, instance, publication):
        with self.lock:
            result = self.socket.request(dict(type='open', instanceId=instance))
            assert result['ok'], result
            self.opened = result
            self.publication = publication
            self.feedback = FeedbackState(result, publication)
            self.sequence = self.heartbeat = 0
            controls = json.loads(publication['canonicalDocument'])['ui']['controls']
            values = {control['id']: False if control['kind'] == 'button' else control.get('fallback', -1) for control in controls}
            self.consume(self.socket.request(self.message('baseline', sequence='0', values=values)))

    def input(self, changes):
        with self.lock:
            assert self.failure is None, self.failure
            self.sequence += 1
            self.consume(self.socket.request(self.message('input', sequence=str(self.sequence), changes=changes)))

    def resync(self):
        with self.lock:
            self.resync_sequence += 1
            self.feedback = FeedbackState(self.opened, self.publication)
            self.consume(self.socket.request(self.message('feedback-resync', sequence=str(self.resync_sequence))))

    def snapshot(self):
        with self.lock:
            assert self.failure is None, self.failure
            return self.feedback.snapshot() if self.feedback else {}


def integration(service, command, **fields):
    reply = service.request(command, **fields)
    assert reply['ok'], reply
    return reply['state']['integration']


def feedback(service, action='inspect', instance=None, publication=None, **fields):
    payload = dict(action=action, **fields)
    if instance is not None:
        payload['instanceId'] = instance
    if publication is not None:
        payload['publicationHash'] = publication['publicationHash']
    reply = c4.operator(service, 'companion_feedback_routes', **payload)
    return reply['feedbackRoutes']


def binding(output, source, **transform):
    return dict(channelId=f'flight-channel-{output}', outputId=f'flight-output-{output}', source=source, **transform)


def bounded_text_publication(original):
    document = json.loads(original['canonicalDocument'])
    page = document['ui']['pages'][0]['id']
    widgets = [widget for widget in document['presentation']['widgets'] if widget['controlIds']]
    outputs = []
    for index in range(64):
        output = f'max-output-{index}'
        outputs.append(dict(id=output, channelId=f'max-channel-{index}', kind='text', label='Decoder label'))
        widgets.append(dict(
            id=f'max-widget-{index}', kind='text', label='Decoder label', pageId=page,
            bounds=dict(x=0.02, y=0.74, width=0.3, height=0.21),
            controlIds=[], feedbackIds=[output], config=dict(prefix='', suffix='')))
    document['ui']['title'] = 'Bounded feedback acceptance'
    document['ui']['feedbackWidgets'] = outputs
    document['presentation']['widgets'] = widgets
    for row in document['ui']['pages']:
        row['items'] = [
            dict(kind=kind, refId=ref, bounds=widget['bounds'])
            for widget in widgets if widget['pageId'] == row['id']
            for kind, refs in [('control', widget['controlIds']), ('feedback', widget['feedbackIds'])]
            for ref in refs
        ]
    canonical = json.dumps(document, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
    return dict(canonicalDocument=canonical, publicationHash='sha256:' + hashlib.sha256(canonical.encode()).hexdigest(), assets=[])


def synthetic_config():
    return dict(document=dict(schemaVersion=2, id='feedback-synthetic', name='Feedback acceptance synthetic source', provider='assetto-corsa-rally', context={}, roles=[], inputs=[], outputs=[], feedbackOutputs=[dict(id='rpm', channel='rpm', output=0)]), connection=dict(source='synthetic', freshnessMs=500), assignments=[])


def synthetic_frame(packet, rpm):
    physics = bytearray((ROOT / 'tests/fixtures/acr/live.physics.bin').read_bytes())
    struct.pack_into('<I', physics, 0, packet)
    struct.pack_into('<i', physics, 20, rpm)
    return dict(sourceId='synthetic', sourceEpoch='1', physics=list(physics), graphics=list((ROOT / 'tests/fixtures/acr/live.graphics.bin').read_bytes()), staticPage=list((ROOT / 'tests/fixtures/acr/live.static.bin').read_bytes()))


def source_invariant(service, instances):
    host = c4.operator(service, 'companion_status')['host']
    owners = {row['instanceId']: {key: row[key] for key in ['companionId', 'publicationHash', 'connected', 'armed', 'generation', 'sessionId', 'values']} for row in host['instances'] if row['instanceId'] in instances}
    sources = service.mapping('mapping_companion_sources')['companionSources']
    counters = {row['id']: (row['generation'], row['sequence']) for row in sources if row['id'] in instances}
    cursors = {instance: c4.wait_for(lambda i=instance: c4.route(service, i)['session'].get('recordingCursor'), 'Recording inspector remained busy') for instance in instances}
    return dict(owners=owners, counters=counters, recordingCursors=cursors)


def wait_sample(peer, output, predicate):
    channel = f'flight-channel-{output}'
    try:
        return c4.wait_for(lambda: (lambda value: value if value and predicate(value) else None)(peer.snapshot().get(channel)), f'{peer.name} output {output} did not reach expected typed feedback')
    except AssertionError as error:
        raise AssertionError(f'{error}; current sample: {peer.snapshot().get(channel)!r}') from error


def dcs_config(port, command_port):
    return dict(document=dict(schemaVersion=2, id='feedback-dcs', name='Injected DCS decoder acceptance', provider='dcs-bios', context=dict(aircraftModule='A-10C'), roles=[], inputs=[], outputs=[], feedbackOutputs=[dict(id='caution', channel='MASTER_CAUTION', output=0), dict(id='frequency', channel='UHF_FREQUENCY', output=0), dict(id='volume', channel='UHF_VOL', output=0)]), connection=dict(cataloguePath=str((ROOT / 'simulation/dcs-bios/catalogue').resolve()), commandHost='127.0.0.1', commandPort=command_port, listenAddress='127.0.0.1', listenPort=port, multicastGroup='', multicastInterface='127.0.0.1', freshnessMs=500), assignments=[])


def dcs_packet(counter_value, volume=32768, frequency='251.000', caution=True):
    assert 0 <= volume <= 65535 and len(frequency.encode('ascii')) == 7
    blocks = [(64, b'A-10C' + b' ' * 19), (4672, struct.pack('<H', 4 if caution else 0)), (4704, frequency.encode('ascii') + b'\0'), (4720, struct.pack('<H', volume)), (65534, struct.pack('<H', counter_value % 256))]
    return b'\x55' * 4 + b''.join(struct.pack('<HH', address, len(data)) + data for address, data in blocks)


class DcsFeed:
    def __init__(self, port):
        self.stop = threading.Event()
        self.port = port
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
            counter_value = 1
            while not self.stop.wait(0.05):
                sender.sendto(dcs_packet(counter_value), ('127.0.0.1', self.port))
                counter_value += 1

    def close(self):
        self.stop.set()
        self.thread.join(timeout=2)
        assert not self.thread.is_alive(), 'Bounded DCS fixture did not stop'


def unused_port(kind=socket.SOCK_STREAM):
    with socket.socket(socket.AF_INET, kind) as probe:
        probe.bind(('127.0.0.1', 0))
        return probe.getsockname()[1]


def assert_capture(service, publication, instances):
    captured = c4.operator(service, 'companion_game_capture', instanceIds=instances)['capture']
    assert captured['schemaVersion'] == 1
    rows = {row['instanceId']: row for row in captured['assignments']}
    assert set(rows) == set(instances)
    for instance, row in rows.items():
        assert set(row) == {'instanceId', 'companionId', 'publicationHash', 'capabilityFingerprint'}
        assert row['companionId'] == instance and row['publicationHash'] == publication['publicationHash'] and row['capabilityFingerprint'] == publication['capabilityFingerprint']
    return captured


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--service', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    publication = json.loads((ROOT / 'crates/companion-service/tests/fixtures/c1-flight-publication.json').read_text())
    evidence = dict(qualification=False, physicalGameQualification=False, mobileQualification=False, virtualReportsSent=False, driverLayoutsChanged=False, peers='two simulated peers over real loopback pinned-CA WSS', confirmedTelemetryEvidence='injected DCS-BIOS UDP fixture through production decoder; no live game claim', checks=[])
    with tempfile.TemporaryDirectory(prefix='osg-companion-feedback-acceptance-') as temporary:
        service = c4.mapping.Service(args.service.resolve(), Path(temporary))
        peers, feed = [], None
        try:
            status = service.mapping('mapping_status')
            assert not status['running'] and not status['virtualDevices']['enabled'] and not status['virtualDevices']['qualified']
            c4.operator(service, 'companion_enable', address='127.0.0.1', advertisedHost='127.0.0.1', port=unused_port(), discovery=False)
            c4.publish(service, publication)
            instances = ['feedback-left', 'feedback-right']
            for instance in instances:
                peer = FeedbackPeer(service, instance)
                peer.publication = publication
                peers.append(peer)
                c4.operator(service, 'companion_assign', companionId=instance, instanceId=instance, publicationHash=publication['publicationHash'])
            capture = assert_capture(service, publication, instances)
            assert all(not row['armed'] for row in c4.operator(service, 'companion_status')['host']['instances']) and not service.mapping('mapping_status')['running']
            evidence['checks'].append('read-only game capture returns immutable IDs/hashes without opening or arming input')
            integration(service, 'integration_save', integrationConfig=synthetic_config())
            assert not integration(service, 'integration_get_state')['running']
            requested = [binding(0, dict(kind='requested', controlId='flight-input-0')), binding(3, dict(kind='host-accepted', controlId='flight-input-0')), binding(2, dict(kind='integration', bindingId='rpm'), scale=0.001), binding(4, dict(kind='host-submitted', controlId='flight-input-2'), scale=50, offset=50)]
            for peer, instance in zip(peers, instances):
                feedback(service, 'save', instance, publication, bindings=requested)
                c4.route(service, instance, 'save', publicationHash=publication['publicationHash'], bindings=[])
                peer.open(instance, publication)
                c4.route(service, instance, 'start', mode='recording')
            peers[0].input([dict(controlId='flight-input-0', value=True)])
            wait_sample(peers[0], 0, lambda s: s.get('value') is True and s['provenance'] == 'requested' and s['status'] == 'fresh')
            wait_sample(peers[0], 3, lambda s: s.get('value') is True and s['provenance'] == 'host-accepted')
            wait_sample(peers[1], 0, lambda s: s.get('value') is False)
            wait_sample(peers[0], 4, lambda s: s['status'] == 'unknown' and 'value' not in s)
            evidence['checks'].append('same-UI instances keep independent requested/accepted values; recording is never claimed submitted')
            integration(service, 'integration_start')
            before = source_invariant(service, instances)
            for packet in [20, 21]:
                integration(service, 'integration_simulate', telemetryFrame=synthetic_frame(packet, 6300))
            for peer in peers:
                wait_sample(peer, 2, lambda s: math.isclose(s.get('value', -1), 6.3) and s['provenance'] == 'local' and s['status'] == 'fresh')
            assert source_invariant(service, instances) == before
            spoof=peers[0].snapshot()['flight-channel-2'].copy()
            spoof['provenance']='game-confirmed'
            spoof['sourceKind']='simulator'
            assert not service.request('companion_feedback',companion=dict(instanceId=instances[0],values=[spoof]))['ok']
            with peers[0].lock:
                assert not peers[0].socket.request(peers[0].message('feedback',values=[spoof]))['ok']
                stale=peers[0].message('feedback-resync',sequence='1')
                stale['sessionId']=peers[1].opened['sessionId']
                assert not peers[0].socket.request(stale)['ok']
            invalid=[dict(requested[0],outputId='flight-output-3')]
            assert not service.request('companion_feedback_routes',companion=dict(action='save',instanceId=instances[0],publicationHash=publication['publicationHash'],bindings=invalid))['ok']
            assert source_invariant(service,instances)==before
            wait_sample(peers[0],2,lambda sample: sample['provenance']=='local')
            evidence['checks'].append('operator/WSS truth spoofing, cross-session resync and wrong output binding are rejected without cross-client effects')
            evidence['checks'].append('native source-only synthetic numeric feedback stays Local and emits no control input')
            feedback(service, 'reset', instances[0], publication)
            wait_sample(peers[0], 0, lambda s: s['status'] == 'unknown' and 'value' not in s)
            wait_sample(peers[1], 0, lambda s: s.get('value') is False and s['status'] == 'fresh')
            feedback(service, 'resync', instances[0], publication)
            wait_sample(peers[0], 0, lambda s: s.get('value') is True and s['status'] == 'fresh')
            peers[0].resync()
            assert source_invariant(service, instances) == before
            evidence['checks'].append('operator reset/resync and wire snapshot resync leave source counters/values and recording cursors unchanged')
            wait_sample(peers[0], 2, lambda s: s['status'] == 'unknown' and 'value' not in s and s['provenance'] == 'local')
            evidence['checks'].append('expired synthetic telemetry is Unknown/Local with no retained value or false confirmation')
            integration(service, 'integration_stop')
            for peer in peers:
                wait_sample(peer, 2, lambda s: s['status'] == 'unknown' and 'value' not in s)
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as commands:
                commands.bind(('127.0.0.1', 0))
                commands.setblocking(False)
                port = unused_port(socket.SOCK_DGRAM)
                integration(service, 'integration_save', integrationConfig=dcs_config(port, commands.getsockname()[1]))
                for peer in peers:
                    wait_sample(peer, 2, lambda s: s['status'] == 'unknown' and 'value' not in s)
                configured = [binding(0, dict(kind='requested', controlId='flight-input-0')), binding(3, dict(kind='host-accepted', controlId='flight-input-0')), binding(1, dict(kind='integration', bindingId='frequency')), binding(2, dict(kind='integration', bindingId='volume'), scale=100/65535)]
                for instance in instances:
                    feedback(service, 'save', instance, publication, bindings=configured)
                integration(service, 'integration_start')
                wait_sample(peers[0], 1, lambda s: s['status'] == 'unknown')
                before = source_invariant(service, instances)
                feed = DcsFeed(port)
                for peer in peers:
                    wait_sample(peer, 1, lambda s: s.get('value') == '251.000' and s['status'] == 'fresh' and s['provenance'] == 'game-confirmed' and s['sourceKind'] == 'simulator')
                    wait_sample(peer, 2, lambda s: math.isclose(s.get('value', -1), 32768*100/65535) and s['status'] == 'fresh' and s['unit'] == '%' and s['range'] == dict(min=0, max=100))
                assert source_invariant(service, instances) == before
                evidence['checks'].append('production DCS-BIOS UDP decoder routes injected typed numeric/text telemetry with confirmed provenance and exact units/ranges')
                peers[0].paused.set()
                time.sleep(0.4)
                wait_sample(peers[1], 1, lambda s: s['status'] == 'fresh')
                peers[0].paused.clear()
                wait_sample(peers[0], 1, lambda s: s['status'] == 'fresh')
                assert len(peers[0].snapshot()) <= 64
                evidence['checks'].append('temporarily unread peer coalesces bounded latest state while the other peer remains fresh')
                peers[0].paused.set()
                peers[0].end()
                bounded=bounded_text_publication(publication)
                c4.publish(service,bounded)
                c4.operator(service,'companion_assign',companionId=instances[0],instanceId=instances[0],publicationHash=bounded['publicationHash'])
                peers[0].open(instances[0],bounded)
                expanded=[dict(channelId=f'max-channel-{i}',outputId=f'max-output-{i}',source=dict(kind='integration',bindingId='frequency')) for i in range(64)]
                feedback(service,'save',instances[0],bounded,bindings=expanded)
                peers[0].paused.clear()
                c4.wait_for(lambda: len(peers[0].snapshot())==64 and all(s.get('value')=='251.000' and s['status']=='fresh' for s in peers[0].snapshot().values()),'64-channel decoder feedback did not reach the peer')
                wait_sample(peers[1],1,lambda sample: sample['status']=='fresh')
                assert peers[0].max_feedback_bytes>c4.FRAME_BYTES and peers[0].max_feedback_bytes<=FEEDBACK_BYTES
                assert not c4.route(service,instances[0])['session']['running']
                before=source_invariant(service,instances)
                evidence['checks'].append('64-channel snapshot exceeds 16KiB logically, uses bounded physical WSS chunks, and preserves the other peer')
                peers[0].paused.set()
                c4.wait_for(lambda: peers[0].snapshot().get('max-channel-0',{}).get('status')=='stale','Paused reader did not age retained feedback to stale')
                wait_sample(peers[1], 1, lambda sample: sample['status']=='fresh')
                peers[0].paused.clear()
                c4.wait_for(lambda: peers[0].snapshot().get('max-channel-0',{}).get('status')=='fresh','Resumed reader did not receive fresh feedback')
                evidence['checks'].append('retained receiver values become Stale at 1000ms without changing source provenance, while the independent peer remains Fresh')
                feed.close()
                feed = None
                c4.wait_for(lambda: peers[0].snapshot().get('max-channel-0',{}).get('status')=='unknown','Expired decoder feedback did not become unknown')
                wait_sample(peers[1], 1, lambda sample: sample['status']=='unknown' and 'value' not in sample)
                assert source_invariant(service, instances) == before
                evidence['checks'].append('expired decoder telemetry becomes Unknown without fabricated fresh confirmation or controller input')
                integration(service, 'integration_stop')
                c4.wait_for(lambda: peers[0].snapshot().get('max-channel-0',{}).get('status')=='unknown','Large stopped decoder feedback did not become unknown')
                wait_sample(peers[1], 1, lambda sample: sample['status']=='unknown' and 'value' not in sample)
                try:
                    unexpected = commands.recv(4096)
                except BlockingIOError:
                    unexpected = b''
                assert not unexpected, 'Source-only acceptance unexpectedly sent a simulator command'
            peers[0].end()
            c4.wait_for(lambda: c4.route(service, instances[0])['session']['cleanupComplete'], 'Disconnected mapping did not clean up')
            assert c4.route(service, instances[1])['session']['running']
            wait_sample(peers[1], 0, lambda s: s.get('value') is False and s['status'] == 'fresh')
            peers[0].open(instances[0], peers[0].publication)
            assert not c4.route(service, instances[0])['session']['running'], 'Feedback reconnect must not restart mapping'
            c4.operator(service, 'companion_revoke', companionId=instances[0])
            c4.wait_for(lambda: len(c4.operator(service, 'companion_status')['host']['instances']) == 1, 'Revocation did not remove only its assignment')
            assert c4.route(service, instances[1])['session']['running']
            wait_sample(peers[1], 0, lambda s: s['status'] == 'fresh' and s.get('value') is False)
            assert_capture(service, publication, instances[1:])
            assert capture['assignments'][0]['publicationHash'] == publication['publicationHash']
            evidence['checks'].append('source close, reconnect and revocation affect only their instance; captured immutable refs remain unchanged')
            for peer in peers:
                assert {'snapshot', 'delta'} <= peer.modes
            evidence['feedbackTransport'] = dict(physicalFrameLimitBytes=c4.FRAME_BYTES, logicalFeedbackLimitBytes=FEEDBACK_BYTES, maxObservedLogicalBytes=max(peer.max_feedback_bytes for peer in peers), observedModes=sorted(set().union(*(peer.modes for peer in peers))))
            final = service.mapping('mapping_status')
            assert not final['virtualDevices']['enabled'] and not final['virtualDevices']['qualified']
            evidence['checks'].append('listener/feedback use no optional live output enablement, virtual reports or provisioning')
        finally:
            if feed:
                feed.close()
            for peer in peers:
                peer.close()
            service.close()
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(evidence, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    main()
