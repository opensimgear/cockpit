#!/usr/bin/env python3
"""Applied unified profile -> real WSS typed feedback. No game context or virtual output is invented."""
import argparse
import copy
import importlib.util
import hashlib
import json
import math
import os
from pathlib import Path
import socket
import struct
import tempfile
import threading
import time

ROOT = Path(os.environ.get("OSG_ACCEPTANCE_ROOT",Path(__file__).resolve().parents[1]))
spec = importlib.util.spec_from_file_location('companion_mapping_acceptance', Path(__file__).with_name('companion-mapping-acceptance.py'))
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
                    result=self.socket.request(self.message('heartbeat',sequence=str(self.heartbeat)))
                    if not result['ok'] and result.get('error')=='Session is obsolete; explicitly reconnect.':
                        self._open_locked(self.instance,self.publication)
                    else:self.consume(result)
                except Exception as error:
                    self.failure = error
                    return

    def _open_locked(self,instance,publication):
        result=self.socket.request(dict(type='open',instanceId=instance,publicationHash=publication['publicationHash']))
        assert result['ok'],result
        self.opened=result;self.publication=publication;self.feedback=FeedbackState(result,publication)
        self.sequence=self.heartbeat=0
        controls=json.loads(publication['canonicalDocument'])['ui']['controls']
        values={control['id']:False if control['kind']=='button' else control.get('fallback',-1) for control in controls}
        self.consume(self.socket.request(self.message('baseline',sequence='0',values=values)))

    def open(self,instance,publication):
        with self.lock:
            self.instance,self.publication=instance,publication
            self._open_locked(instance,publication)

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


def dcs_packet(counter_value, volume=32768, frequency='251.000', caution=True):
    assert 0 <= volume <= 65535 and len(frequency.encode('ascii')) == 7
    blocks = [(64, b'A-10C' + b' ' * 19), (4672, struct.pack('<H', 4 if caution else 0)), (4704, frequency.encode('ascii') + b'\0'), (4720, struct.pack('<H', volume)), (65534, struct.pack('<H', counter_value % 256))]
    return b'\x55' * 4 + b''.join(struct.pack('<HH', address, len(data)) + data for address, data in blocks)


def unused_port():
    with socket.socket() as probe:probe.bind(('127.0.0.1',0));return probe.getsockname()[1]


def feedback_profile(service,configurations,large=False):
    document,assignments,models=c4.companion_profile(service,configurations,read_inputs=not large)
    document['name']='Unified profile feedback'
    for index,(instance,publication) in enumerate(configurations):
        model=models[instance];resource=model['resource'];assignment=model['assignment']
        values={};constants={}
        for number,observed in enumerate(port for port in model['ports'] if port['descriptor']['direction']=='input'):
            descriptor=observed['descriptor'];contract=descriptor['contract'];schema=contract['schema'];kind=schema['type'];logical=f'feedback{number}'
            c4.add_port(document,assignment,resource,logical,observed)
            if kind=='boolean' and not large:
                source=dict(nodeId=resource['id'],portId='button')
            else:
                value=False if kind=='boolean' else ('L'*256 if large else 'Profile local') if kind=='text' else 42
                key=json.dumps([contract,value],sort_keys=True)
                if key not in constants:
                    node=f'constant{index}-{len(constants)}';constants[key]=node
                    document['processingNodes'].append(dict(id=node,name=node,operation=dict(kind='constant',value=value),inputs=[],outputs=[c4.mapping.port('value',contract,'output')]))
                source=dict(nodeId=constants[key],portId='value')
            document['bindings'].append(dict(id=f'feedback{index}-{number}',**{'from':source,'to':dict(nodeId=resource['id'],portId=logical)}))
    return document,assignments,models


def wait_channel(peer,channel,predicate):
    return c4.wait_for(lambda:(lambda sample:sample if sample and predicate(sample) else None)(peer.snapshot().get(channel)),f'{peer.name}/{channel} did not reach expected profile feedback')


def wait_sample(peer,output,predicate):return wait_channel(peer,f'flight-channel-{output}',predicate)


def local_fresh(value):return lambda sample:sample.get('value')==value and sample['status']=='fresh' and sample['provenance']=='local' and sample['sourceKind']=='host'


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--service',type=Path,required=True);parser.add_argument('--output',type=Path);args=parser.parse_args()
    publication=json.loads((ROOT/'crates/companion-service/tests/fixtures/c1-flight-publication.json').read_text())
    evidence=dict(qualification=False,physicalGameQualification=False,mobileQualification=False,virtualReportsSent=False,driverLayoutsChanged=False,peers='two simulated peers over real loopback pinned-CA WSS',feedbackEvidence='actual native reducer local/host state; no simulator confirmation claim',checks=[])
    with tempfile.TemporaryDirectory(prefix='osg-profile-feedback-') as temporary:
        service=c4.mapping.Service(args.service.resolve(),Path(temporary).resolve());peers=[]
        try:
            c4.operator(service,'companion_enable',address='127.0.0.1',advertisedHost='127.0.0.1',port=unused_port(),discovery=False)
            c4.publish(service,publication);instances=['feedback-left','feedback-right']
            for instance in instances:
                peer=FeedbackPeer(service,instance);peers.append(peer)
                c4.operator(service,'companion_assign',companionId=instance,instanceId=instance,publicationHash=publication['publicationHash']);peer.open(instance,publication)
            document,assignments,models=feedback_profile(service,[(instance,publication) for instance in instances])
            saved=c4.mapping.save(service,document,assignments);owner=c4.mapping.apply(service,saved)
            for peer in peers:
                peer.input([dict(controlId='flight-input-0',value=True)])
                wait_sample(peer,0,local_fresh(True));wait_sample(peer,1,local_fresh('Profile local'));wait_sample(peer,2,local_fresh(42))
                assert peer.snapshot()['flight-channel-2']['unit']=='%' and peer.snapshot()['flight-channel-2']['range']==dict(min=0,max=100)
            evidence['checks'].append('actual applied profile routes exact native input and typed local text/numeric feedback with original widget units/ranges over real WSS')
            before=peers[1].snapshot()
            with peers[1].lock:
                forged=peers[0].message('input',sequence='999',changes=[dict(controlId='flight-input-0',value=False)])
                assert not peers[1].socket.request(forged)['ok']
            assert local_fresh(True)(peers[1].snapshot()['flight-channel-0'])
            assert not service.request('companion_feedback',companion=dict(instanceId=instances[0],values=[dict(value=True)]))['ok']
            c4.mapping.retired_boundaries(service)
            assert c4.mapping.inspect(service,owner)['owner']==owner
            evidence['checks'].append('cross-peer input identity and browser feedback spoof rejected without replacing applied native owner')
            peers[0].paused.set()
            c4.wait_for(lambda:peers[0].snapshot().get('flight-channel-0',{}).get('status')=='stale','Paused receiver failed to age retained values')
            wait_sample(peers[1],0,local_fresh(True));peers[0].paused.clear()
            service.profiles('profile_stop');assert not service.request('profile_runtime_inspect',owner=owner)['ok']
            owner=c4.mapping.apply(service,saved)
            for peer in peers:peer.input([dict(controlId='flight-input-0',value=True)]);wait_sample(peer,0,local_fresh(True))
            evidence['checks'].append('one expired/unread peer ages its retained samples while independent peer remains fresh; fresh explicit Apply reacquires its baseline')
            service.profiles('profile_stop')
            assert not service.request('profile_runtime_inspect',owner=owner)['ok']
            bounded=bounded_text_publication(publication);c4.publish(service,bounded)
            c4.operator(service,'companion_assign',companionId=instances[0],instanceId=instances[0],publicationHash=bounded['publicationHash'])
            peers[0].end();peers[0].open(instances[0],bounded)
            large,local,_=feedback_profile(service,[(instances[0],bounded),(instances[1],publication)],large=True)
            large=c4.mapping.save(service,large,local);large_owner=c4.mapping.apply(service,large)
            c4.wait_for(lambda:len(peers[0].snapshot())==64 and all(local_fresh('L'*256)(sample) for sample in peers[0].snapshot().values()),'64-channel applied feedback did not arrive')
            assert c4.FRAME_BYTES<peers[0].max_feedback_bytes<=FEEDBACK_BYTES
            wait_sample(peers[1],2,local_fresh(42))
            evidence['checks'].append('64 real profile feedback ports exceed16KiB logically and use bounded chunked WSS without affecting the second peer')
            service.profiles('profile_stop')
            assert not service.request('profile_runtime_inspect',owner=large_owner)['ok']
            # Stop restores assignment ownership; peers explicitly reopen only to the original panel baseline, never an output run.
            c4.wait_for(lambda:not any(sample['status']=='fresh' for sample in peers[0].snapshot().values()),'Stopped feedback retained fresh applied state')
            for peer in peers:peer.close()
            peers.clear();c4.operator(service,'companion_revoke',companionId=instances[0])
            assert {row['instanceId'] for row in c4.operator(service,'companion_status')['host']['instances']}=={instances[1]}
            evidence['checks'].append('Stop fences old native inspection/feedback owner; revocation preserves other paired instance')
            c4.operator(service,'companion_disable')
        finally:
            for peer in peers:peer.close()
            service.close()
    if args.output:args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(evidence,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(evidence,indent=2))


if __name__=='__main__':main()
