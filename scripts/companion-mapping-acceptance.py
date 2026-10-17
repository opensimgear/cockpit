#!/usr/bin/env python3
"""Exercise real WSS peers through one applied unified profile. No virtual output."""
import argparse
import base64
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import socket
import ssl
import struct
import tempfile
import threading
import time
import uuid
from urllib.parse import urlsplit

ROOT = Path(os.environ.get("OSG_ACCEPTANCE_ROOT", Path(__file__).resolve().parents[1]))
spec = importlib.util.spec_from_file_location("mapping_acceptance", Path(__file__).with_name("mapping-acceptance.py"))
mapping = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mapping)
FRAME_BYTES = 16 * 1024
TRANSFER_BYTES = 13 * 1024 * 1024
PROTOCOL = "opensimgear-companion-v1"


def encoded_frame(value, opcode=1):
    payload = value if isinstance(value, bytes) else json.dumps(value, separators=(",", ":")).encode()
    assert len(payload) <= FRAME_BYTES, "Acceptance peer exceeded frame budget"
    mask = os.urandom(4)
    size = len(payload)
    prefix = bytes([0x80 | opcode, 0x80 | size]) if size < 126 else bytes([0x80 | opcode, 0x80 | 126]) + struct.pack("!H", size)
    return prefix + mask + bytes(value ^ mask[index % 4] for index, value in enumerate(payload))


class Socket:
    def __init__(self, pairing):
        address = urlsplit(pairing["endpoint"])
        assert address.scheme == "wss" and address.hostname == "127.0.0.1" and address.path == "/companion" and not address.query
        # Exact CA arrives over isolated private operator IPC; this simulated peer
        # is not evidence for the native client's separate out-of-band SPKI check.
        certificate = ssl.DER_cert_to_PEM_cert(base64.b64decode(pairing["caCertificate"], validate=True))
        context = ssl.create_default_context(cadata=certificate)
        self.socket = context.wrap_socket(socket.create_connection((address.hostname, address.port), timeout=3), server_hostname=address.hostname)
        self.buffer = bytearray()
        key = base64.b64encode(os.urandom(16)).decode()
        self.socket.sendall((f"GET /companion HTTP/1.1\r\nHost: {address.netloc}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: {PROTOCOL}\r\n\r\n").encode())
        while b"\r\n\r\n" not in self.buffer:
            chunk = self.socket.recv(4096)
            if not chunk:
                raise RuntimeError("WSS upgrade closed unexpectedly")
            self.buffer.extend(chunk)
            assert len(self.buffer) <= 32768, "Unbounded handshake"
        header, tail = self.buffer.split(b"\r\n\r\n", 1)
        self.buffer = bytearray(tail)
        lines = header.decode("ascii").split("\r\n")
        fields = {name.lower(): value.strip() for name, value in (line.split(":", 1) for line in lines[1:])}
        expected = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        assert lines[0].startswith("HTTP/1.1 101 ") and fields["sec-websocket-accept"] == expected and fields["sec-websocket-protocol"] == PROTOCOL

    def exact(self, size):
        while len(self.buffer) < size:
            chunk = self.socket.recv(min(8192, size - len(self.buffer)))
            if not chunk:
                raise RuntimeError("WSS peer closed unexpectedly")
            self.buffer.extend(chunk)
        result = bytes(self.buffer[:size])
        del self.buffer[:size]
        return result

    def frame(self):
        first, second = self.exact(2)
        assert first & 0x80 and not first & 0x70 and not second & 0x80, "Unexpected fragmented/masked server frame"
        size = second & 0x7f
        if size == 126:
            size = struct.unpack("!H", self.exact(2))[0]
        elif size == 127:
            size = struct.unpack("!Q", self.exact(8))[0]
        assert size <= FRAME_BYTES, "Server frame exceeded budget"
        payload = self.exact(size)
        opcode = first & 15
        if opcode == 9:
            self.socket.sendall(encoded_frame(payload, 10))
            return self.frame()
        assert opcode == 1, "Expected bounded JSON text frame"
        return json.loads(payload)

    def request(self, value):
        self.socket.sendall(encoded_frame(value))
        result = self.frame()
        if result.get("type") != "transfer-start":
            return result
        assert 0 < result["bytes"] <= TRANSFER_BYTES
        transferred = bytearray()
        index = 0
        while True:
            chunk = self.frame()
            assert chunk["id"] == result["id"]
            if chunk["type"] == "transfer-end":
                break
            assert chunk["type"] == "transfer-chunk" and chunk["index"] == index
            transferred.extend(base64.b64decode(chunk["base64"], validate=True))
            assert len(transferred) <= result["bytes"]
            index += 1
        assert len(transferred) == result["bytes"] and "sha256:" + hashlib.sha256(transferred).hexdigest() == result["sha256"]
        return json.loads(transferred)

    def close(self):
        self.socket.close()


class Peer:
    def __init__(self, service, name):
        self.name = name
        pairing = json.loads(operator(service, "companion_pair_offer")["pairing"]["manualPayload"])
        self.socket = Socket(pairing)
        reply = self.socket.request(dict(type="pair", secret=pairing["secret"], companionId=name, name=name))
        assert reply["ok"], reply
        operator(service, "companion_approve", requestId=reply["requestId"], companionId=name)
        paired = self.socket.request(dict(type="pair-status", requestId=reply["requestId"], secret=pairing["secret"]))
        assert paired["ok"] and paired["hostId"] == pairing["hostId"] and paired["companionId"] == name
        assert self.socket.request(dict(type="auth", token=paired["token"]))["ok"]
        self.lock = threading.Lock()
        self.stopped = threading.Event()
        self.paused = threading.Event()
        self.failure = None
        self.opened = None
        self.sequence = 0
        self.heartbeat = 0
        self.thread = threading.Thread(target=self.renew, daemon=True)
        self.thread.start()

    def renew(self):
        while not self.stopped.wait(0.2):
            if self.paused.is_set():
                continue
            with self.lock:
                if self.opened is None:
                    continue
                self.heartbeat += 1
                try:
                    result = self.socket.request(self.message("heartbeat", sequence=str(self.heartbeat)))
                    if not result['ok'] and result.get('error') == 'Session is obsolete; explicitly reconnect.':
                        self._open_locked(self.instance, self.publication)
                    else: assert result['ok'],result
                except Exception as error:
                    self.failure = error
                    return

    def message(self, kind, **fields):
        return dict(version=1, type=kind, **{key: self.opened[key] for key in ["clientId", "sessionId", "generation", "publicationHash"]}, **fields)

    def _open_locked(self, instance, publication):
        result=self.socket.request(dict(type='open',instanceId=instance,publicationHash=publication['publicationHash']))
        assert result['ok'],result
        self.opened=result;self.sequence=self.heartbeat=0
        controls=json.loads(publication['canonicalDocument'])['ui']['controls']
        values={control['id']:False if control['kind']=='button' else control.get('fallback',-1) for control in controls}
        assert self.socket.request(self.message('baseline',sequence='0',values=values))['ok']

    def open(self, instance, publication):
        with self.lock:
            self.instance,self.publication=instance,publication
            self._open_locked(instance,publication)

    def input(self, changes):
        with self.lock:
            assert self.failure is None, self.failure
            self.sequence += 1
            result = self.socket.request(self.message("input", sequence=str(self.sequence), changes=changes))
            assert result["ok"], result

    def end(self):
        with self.lock:
            if self.opened:
                result = self.socket.request(self.message("close"))
                assert result["ok"], result
                self.opened = None

    def close(self):
        self.stopped.set()
        self.thread.join(timeout=4)
        self.socket.close()


def operator(service, command, **payload):
    reply = service.request(command, companion=payload)
    assert reply["ok"], reply
    return reply["state"]["companions"]


def publish(service, publication):
    payload = json.dumps({key: publication[key] for key in ["canonicalDocument", "publicationHash", "assets"]}, separators=(",", ":")).encode()
    upload = operator(service, "companion_publish_begin", size=len(payload))["uploadId"]
    for offset in range(0, len(payload), 8192):
        operator(service, "companion_publish_chunk", uploadId=upload, offset=offset, bytes=list(payload[offset:offset + 8192]))
    operator(service, "companion_publish_commit", uploadId=upload)


wait_for = mapping.wait_for


def sample(publication):
    controls=json.loads(publication['canonicalDocument'])['ui']['controls']
    return [dict(controlId=next(control['id'] for control in controls if control['kind']==kind),value=value) for kind,value in [('button',True),('axis',.75),('hat',7)] if any(control['kind']==kind for control in controls)]


def axis_value(value):
    scaled=value*(32768 if value<0 else 32767)
    return math.ceil(scaled-.5) if scaled<0 else math.floor(scaled+.5)


def resources(service):
    result=[];offset=0
    while True:
        page=mapping.state(service,'profile_resources_list','profileResources',offset=offset,limit=128)
        assert page['offset']==offset and len(page['resources'])<=128
        result.extend(page['resources']);offset+=len(page['resources'])
        if offset>=page['total']:return result
        assert page['resources'],'Inventory paging made no progress'


def selected_resource(service,instance):
    for resource in resources(service):
        if resource['resourceType']!='companion':continue
        detail=mapping.state(service,'profile_resources_inspect','profileResources',resourceId=resource['id'],offset=0,limit=128)
        if any(item==dict(kind='companion-instance',value=instance) for item in detail['evidence']):
            assert detail['total']<=128,'Acceptance publication unexpectedly exceeds selected descriptor budget'
            assert all(port['current'] for port in detail['ports']),detail
            return detail
    return None


def add_port(document,assignment,resource,logical,observed):
    descriptor=observed['descriptor']
    resource['ports'].append(mapping.port(logical,descriptor['contract'],descriptor['direction']))
    assignment['ports'].append(dict(profilePortId=logical,observedPortId=descriptor['id'],expectedContractHash=observed['contractHash']))


def companion_profile(service,configurations,read_inputs=True):
    document=mapping.blank('Unified WSS inputs');assignments=[];models={}
    for index,(instance,publication) in enumerate(configurations):
        detail=wait_for(lambda:selected_resource(service,instance),f'No native descriptor for {instance}')
        logical=f'panel{index}'
        resource=dict(id=logical,name=f'Panel {index}',kind='companion',required=False,ports=[])
        assignment=dict(resourceId=logical,sourceId=detail['resource']['id'],ports=[])
        models[instance]=dict(resource=resource,assignment=assignment,ports=detail['ports'],nodes={})
        for change in (sample(publication) if read_inputs else []):
            control=next(control for control in json.loads(publication['canonicalDocument'])['ui']['controls'] if control['id']==change['controlId'])
            kind=control['kind'];observed=next(port for port in detail['ports'] if port['descriptor']['id']==change['controlId'])
            add_port(document,assignment,resource,kind,observed)
            node=f'read{index}-{kind}';contract=observed['descriptor']['contract']
            document['processingNodes'].append(dict(id=node,name=node,operation=dict(kind='identity'),inputs=[mapping.port('input',contract,'input')],outputs=[mapping.port('value',contract,'output')]))
            document['bindings'].append(dict(id=f'bind{index}-{kind}',**{'from':dict(nodeId=logical,portId=kind),'to':dict(nodeId=node,portId='input')}))
            models[instance]['nodes'][kind]=node
        document['resources'].append(resource);assignments.append(assignment)
        document['companionAssignments'].append(dict(resourceId=logical,publicationHash=publication['publicationHash']))
    return document,assignments,models


def reading(service,owner,node):
    page=mapping.inspect(service,owner,node)
    value=next(port for port in page['ports'] if port['direction']=='output')['sample']
    return value


def expected(publication):
    return {next(control['kind'] for control in json.loads(publication['canonicalDocument'])['ui']['controls'] if control['id']==change['controlId']):(axis_value(change['value']) if isinstance(change['value'],float) else change['value']) for change in sample(publication)}


def exact_readings(service,owner,model,values):
    return all(reading(service,owner,model['nodes'][kind])==dict(status='known',value=dict(kind='scalar',value=value)) for kind,value in values.items())


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--service',type=Path,required=True);parser.add_argument('--output',type=Path);args=parser.parse_args()
    flight=json.loads((ROOT/'crates/companion-service/tests/fixtures/c1-flight-publication.json').read_text())
    racing=json.loads((ROOT/'scripts/fixtures/companion-racing-publication.json').read_text())
    configurations=[('flight-left',flight),('flight-right',flight),('racing',racing)]
    evidence=dict(qualification=False,virtualReportsSent=False,driverLayoutsChanged=False,nativeClientPinQualification=False,physicalGameQualification=False,peers='three simulated peers on real loopback pinned-CA WSS',checks=[])
    with tempfile.TemporaryDirectory(prefix='osg-profile-companions-') as temporary:
        directory=Path(temporary).resolve();service=mapping.Service(args.service.resolve(),directory);peers=[]
        try:
            with socket.socket() as probe:probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
            operator(service,'companion_enable',address='127.0.0.1',advertisedHost='127.0.0.1',port=port,discovery=False)
            publish(service,flight);publish(service,racing)
            for instance,publication in configurations:
                peer=Peer(service,instance);peers.append(peer)
                operator(service,'companion_assign',companionId=instance,instanceId=instance,publicationHash=publication['publicationHash']);peer.open(instance,publication)
            document,assignments,models=companion_profile(service,configurations)
            saved=mapping.save(service,document,assignments)
            copied=service.profiles('profile_duplicate',id=saved['id'],name='Portable Copy')['document']
            assert service.profiles('profile_assignments_get',id=copied['id'])['assignments']==[]
            assert not any(instance in json.dumps(copied) for instance,_ in configurations),'Portable profile leaked local instance identity'
            owner=mapping.apply(service,saved)
            for peer,(_,publication) in zip(peers,configurations):peer.input(sample(publication))
            for instance,publication in configurations:wait_for(lambda:exact_readings(service,owner,models[instance],expected(publication)),f'Actual native typed values for {instance} did not arrive')
            evidence['checks'].append('one real applied profile receives three isolated WSS sources and exact native button/axis/hat values through observed contract/hash pins')
            peers[0].end()
            wait_for(lambda:reading(service,owner,models['flight-left']['nodes']['button'])['status']!='known','Closed optional peer retained a Known graph input')
            assert exact_readings(service,owner,models['flight-right'],expected(flight))
            assert exact_readings(service,owner,models['racing'],expected(racing))
            peers[0].open('flight-left',flight)
            time.sleep(.2)
            assert reading(service,owner,models['flight-left']['nodes']['button'])['status']!='known','Reconnect silently reacquired a lost profile input'
            service.profiles('profile_stop')
            assert not service.request('profile_runtime_inspect',owner=owner)['ok']
            fresh=mapping.apply(service,saved);assert fresh!=owner
            peers[0].input(sample(flight));wait_for(lambda:exact_readings(service,fresh,models['flight-left'],expected(flight)),'Fresh explicit Apply did not acquire a new baseline')
            evidence['checks'].append('optional source close preserves independent branches; reconnect stays disarmed until explicit new Apply; old owner cannot inspect')
            service.profiles('profile_stop')
            mapping.retired_boundaries(service)
            operator(service,'companion_revoke',companionId='flight-left')
            retained=operator(service,'companion_status')['host']['instances']
            assert {row['instanceId'] for row in retained}=={'flight-right','racing'}
            evidence['checks'].append('retired recording/feedback start boundaries rejected and revocation preserves the other paired instances')
            for peer in peers:peer.close()
            peers.clear();operator(service,'companion_disable')
        finally:
            for peer in peers:peer.close()
            service.close()
        service=mapping.Service(args.service.resolve(),directory)
        try:
            status=operator(service,'companion_status')
            assert not status['listener']['enabled'] and all(not item['armed'] for item in status['host']['instances'])
            assert service.profiles('profile_status')['profileSession']['owner'] is None
            assert service.profiles('profile_export',id=saved['id'])['document']==saved
            evidence['checks'].append('restart keeps offline intended descriptors/profile document and leaves listener/input/output ownership stopped')
        finally:service.close()
    if args.output:args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(evidence,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(evidence,indent=2))


if __name__=='__main__':main()
