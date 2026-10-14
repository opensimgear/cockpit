#!/usr/bin/env python3
"""Exercise mapping IPC in an isolated real native service; never send virtual reports."""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]

class Service:
    def __init__(self, executable, directory):
        env = dict(os.environ, OSG_DCS_BIOS_CONFIG=str(directory / 'dcs-bios.json'),
                   OSG_GAMES_CONFIG=str(directory / 'games.json'))
        self.client = str(uuid.uuid4())
        self.process = subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.DEVNULL, env=env, cwd=ROOT,
                                        creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        self.replies = queue.Queue()
        def read():
            while True:
                line = self.process.stdout.readline(66562)
                if not line:
                    self.replies.put(RuntimeError('Native service exited'))
                    return
                if len(line) > 66561 or not line.endswith(b'\n'):
                    self.replies.put(RuntimeError('Native response exceeded its frame bound'))
                    return
                try:
                    self.replies.put(json.loads(line))
                except Exception as error:
                    self.replies.put(error)
                    return
        threading.Thread(target=read, daemon=True).start()

    def request(self, command, **fields):
        data = dict(command=command, clientId=self.client, **fields)
        self.process.stdin.write(json.dumps(data, separators=(',', ':')).encode() + b'\n')
        self.process.stdin.flush()
        result = self.replies.get(timeout=10)
        if isinstance(result, Exception):
            raise result
        return result

    def mapping(self, command, **fields):
        result = self.request(command, **fields)
        assert result['ok'], result
        return result['state']['mappings']

    def close(self):
        self.process.stdin.close()
        try:
            code = self.process.wait(timeout=5)
            assert code == 0, code
        finally:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait(timeout=5)

def upload(service, profile):
    data = json.dumps(profile, separators=(',', ':')).encode()
    upload_id = service.mapping('mapping_import_begin', size=len(data))['uploadId']
    for offset in range(0, len(data), 8192):
        service.mapping('mapping_import_chunk', uploadId=upload_id, offset=offset,
                        bytes=list(data[offset:offset + 8192]))
    saved = service.mapping('mapping_import_commit', uploadId=upload_id)
    assert saved['document']['id'] == profile['id']

def assignments(profile):
    return [dict(role=role['id'], source=dict(origin=role['origin'], id=role['id']),
                 controls=[dict(control=c['id'], sourceControl=c['id']) for c in role['controls']])
            for role in profile['roles']]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--service', type=Path, required=True)
    parser.add_argument('--physical', help='Optional Windows VID:PID for read-only physical preview')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    profile = json.loads((ROOT / 'crates/mapping/fixtures/direct-profile.json').read_text())
    profile['id'] = str(uuid.uuid4())
    assigned = assignments(profile)
    evidence = {'synthetic': False, 'restart': False, 'physical': 'not requested',
                'virtualReportsSent': False, 'driverLayoutsChanged': False}
    # Exercise the private production feeder entry with an invalid empty target
    # set. It cannot acquire a slot or send a virtual report.
    rejected = dict(command='start', token=dict(session=str(uuid.uuid4()), generation='1'), bindings=[])
    backend = 'uinput' if sys.platform == 'linux' else 'macos-hid' if sys.platform == 'darwin' else 'vjoy'
    probe = subprocess.run([str(args.service.resolve()), f'--osg-{backend}-feeder'],
                           input=(json.dumps(rejected) + '\n').encode(), stdout=subprocess.PIPE,
                           stderr=subprocess.DEVNULL, timeout=8,
                           creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
    assert probe.returncode == 1, probe.returncode
    lines = probe.stdout.splitlines()
    assert 1 <= len(lines) <= 2 and all(len(line) <= 66560 for line in lines)
    hello = json.loads(lines[0])
    assert hello['backend'] == backend and hello['protocol'] == 1
    if hello['ok']:
        reply = json.loads(lines[1])
        assert not reply['ok'] and reply['token'] == rejected['token'] and not reply['active']
        evidence['feederEntry'] = 'installed driver; empty targets rejected before acquisition'
    else:
        evidence['feederEntry'] = 'optional dependency unavailable; process exited without activation'
    with tempfile.TemporaryDirectory(prefix='osg-mapping-acceptance-') as temporary:
        directory = Path(temporary)
        service = Service(args.service.resolve(), directory)
        try:
            status = service.mapping('mapping_status')
            assert not status['running'] and not status['virtualDevices']['enabled']
            upload(service, profile)
            service.mapping('mapping_assignments_save', profileId=profile['id'], assignments=assigned)
            token = service.mapping('mapping_preview_start', profileId=profile['id'])['token']
            for pressed in [True, False, True, False]:
                service.mapping('mapping_simulate', token=token, source=assigned[0]['source'],
                                changes=[dict(control='trigger', value=dict(kind='button', value=pressed))])
            records = service.mapping('mapping_recording', after='0')['records']
            buttons = [r['report']['state']['buttons'][0] for r in records
                       if r['report']['target'] == 'main' and r['report']['reason'] == 'change']
            assert buttons == [True, False, True, False], buttons
            assert service.mapping('mapping_status')['ready']
            for command in ['mapping_virtual_start', 'mapping_virtual_create', 'mapping_virtual_recover']:
                assert not service.request(command)['ok']
            service.mapping('mapping_stop')
            assert not service.request('mapping_simulate', token=token, source=assigned[0]['source'],
                                       changes=[dict(control='trigger', value=dict(kind='button', value=True))])['ok']
            inspection = service.mapping('mapping_virtual_inspect')
            evidence['inspection'] = inspection
            evidence['synthetic'] = True
            if args.physical:
                vendor, product = [int(part, 16) for part in args.physical.split(':')]
                deadline = time.monotonic() + 8
                selected = None
                while time.monotonic() < deadline:
                    result = service.request('controller_list', offset=0)
                    assert result['ok'], result
                    selected = next((s for s in result['state']['controllers']['sources']
                                     if s['vendorId'] == vendor and s['productId'] == product
                                     and s['online'] and s['baselineReady']), None)
                    if selected:
                        break
                    time.sleep(.1)
                assert selected, 'Requested physical controller has no complete baseline'
                controls = []
                for offset in range(0, selected['controlCount'], 32):
                    result = service.request('controller_inspect', sourceId=selected['id'], offset=offset)
                    assert result['ok'], result
                    controls.extend(c['capability'] for c in result['state']['controllers']['controls'])
                chosen = {kind: next(c for c in controls if c['kind'] == kind)
                          for kind in ['button', 'axis', 'hat']}
                physical = json.loads(json.dumps(profile))
                physical['id'] = str(uuid.uuid4())
                physical['roles'][0]['origin'] = 'host-controller'
                axis_range = 'bipolar' if chosen['axis']['min'] < 0 else 'unipolar'
                physical['roles'][0]['controls'][1]['kind']['range'] = axis_range
                physical['targets'][0]['layout']['axes'][0] = dict(range=axis_range, fallback=0)
                local = assignments(physical)
                local[0]['source']['id'] = selected['id']
                local[0]['controls'] = [dict(control=logical, sourceControl=chosen[kind]['id'])
                                        for logical, kind in [('trigger', 'button'), ('x', 'axis'), ('hat', 'hat')]]
                upload(service, physical)
                service.mapping('mapping_assignments_save', profileId=physical['id'], assignments=local)
                service.mapping('mapping_preview_start', profileId=physical['id'])
                time.sleep(.2)
                physical_status = service.mapping('mapping_status')
                assert physical_status['running'] and physical_status['ready'], physical_status
                service.mapping('mapping_stop')
                evidence['physical'] = {'vidPid': args.physical, 'sourceId': selected['id'],
                                        'controls': chosen, 'ready': True}
        finally:
            service.close()
        restarted = Service(args.service.resolve(), directory)
        try:
            status = restarted.mapping('mapping_status')
            assert not status['running'] and not status['virtualDevices']['enabled']
            saved = restarted.mapping('mapping_export', profileId=profile['id'])['document']
            assert saved == profile
            local = restarted.mapping('mapping_assignments_get', profileId=profile['id'])
            assert len(local['roles']) == 2
            restarted.mapping('mapping_preview_start', profileId=profile['id'])
            assert restarted.mapping('mapping_status')['ready']
            restarted.mapping('mapping_stop')
            evidence['restart'] = True
        finally:
            restarted.close()
    text = json.dumps(evidence, indent=2)
    print(text)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text + '\n', encoding='utf-8')

if __name__ == '__main__':
    main()
