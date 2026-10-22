#!/usr/bin/env python3
"""Exercise unified profile lifecycle and retired boundaries in an isolated native service."""
import argparse
import errno
import json
import os
import platform
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import uuid

ROOT = Path(os.environ.get("OSG_ACCEPTANCE_ROOT", Path(__file__).resolve().parents[1]))
DEFAULT = "00000000-0000-4000-8000-000000000001"

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
                line = self.process.stdout.readline(2 * 1024 * 1024 + 1026)
                if not line:
                    self.replies.put(RuntimeError('Native service exited'))
                    return
                if len(line) > 2 * 1024 * 1024 + 1025 or not line.endswith(b'\n'):
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

    def profiles(self, command, **fields):
        result = self.request(command, **fields)
        assert result['ok'], result
        return result['state']['profiles']

    def close(self):
        self.process.stdin.close()
        try:
            code = self.process.wait(timeout=5)
            assert code == 0, code
        finally:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait(timeout=5)

def state(service, command, key, **fields):
    result = service.request(command, **fields)
    assert result['ok'], result
    return result['state'][key]


def blank(name):
    result = json.loads((ROOT / 'tests/fixtures/profiles/blank.json').read_text(encoding='utf-8'))
    result.update(id=str(uuid.uuid4()), name=name, revision=1)
    return result


def port(id, contract, direction):
    return dict(id=id, name=id, direction=direction, contract=json.loads(json.dumps(contract)))


def constant_profile(name='Native acceptance'):
    result = blank(name)
    contract = dict(delivery='state', schema=dict(type='boolean'), freshnessMs=1000, fallback=False)
    result['processingNodes'] = [dict(id='constant', name='Constant', operation=dict(kind='constant', value=True), inputs=[], outputs=[port('value', contract, 'output')]), dict(id='inverse', name='Inverse', operation=dict(kind='not'), inputs=[port('input', contract, 'input')], outputs=[port('value', contract, 'output')])]
    result['bindings'] = [dict(id='constant-to-inverse', **{'from':dict(nodeId='constant',portId='value'),'to':dict(nodeId='inverse',portId='input')})]
    return result


def save(service, document, assignments=None, expected=None):
    return service.profiles('profile_editor_save', document=document, expectedRevision=expected, assignments=assignments or [])['document']


def apply(service, document):
    result = service.profiles('profile_apply', id=document['id'], expectedRevision=document['revision'])
    run = result['profileSession']
    assert run['state'] == 'running' and run['owner']['profileId'] == document['id'] and run['owner']['profileRevision'] == document['revision'], result
    return run['owner']


def inspect(service, owner, node=None):
    fields = dict(owner=owner, offset=0, limit=64)
    if node is not None: fields['nodeId'] = node
    return state(service, 'profile_runtime_inspect', 'profileInspection', **fields)


def wait_for(predicate, message, seconds=6):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        result = predicate()
        if result: return result
        time.sleep(.02)
    raise AssertionError(message)


def retired_boundaries(service):
    for command in ['mapping_profiles','mapping_import_begin','mapping_assignments_save','mapping_preview_start','mapping_virtual_start','mapping_virtual_create','mapping_stop','mapping_simulate','mapping_companion_route','companion_feedback_routes','dcs_start','dcs_configure','integration_start','integration_save','integration_simulate','game_activate','game_launch','companion_game_capture','controller_visibility_apply','controller_visibility_launch']:
        result = service.request(command, profileId='obsolete', companion=dict(action='start'), capture=dict(resume=True))
        assert not result['ok'], (command,result)
    assert not service.request('profile_apply',id=DEFAULT,expectedRevision=1,owner=dict(session='browser'),resume=True)['ok']


def legacy_bytes(directory):
    files = [directory/'dcs-bios.json',directory/'integration.json',directory/'games.json',directory/'mappings/local-settings.json',directory/'mappings/companion-routes.json',directory/'companions/feedback-routes.json']
    for file in files:
        file.parent.mkdir(parents=True,exist_ok=True)
        file.write_bytes(b'obsolete configuration must stay ignored')
    return {file:file.read_bytes() for file in files}


def assert_preserved(files):
    assert all(file.read_bytes() == expected for file,expected in files.items()), 'Obsolete configuration was read as writable runtime state or overwritten'


def assert_passive_visibility(visibility, host):
    assert not visibility['enabled'] and not visibility['global']['inspected'], visibility
    # Backend qualification is independent of local opt-in and driver presence.
    assert visibility['qualified'] == (host == 'win32'), visibility


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--service',type=Path,required=True)
    parser.add_argument('--physical',help='Optional VID:PID for read-only physical metadata')
    parser.add_argument('--output',type=Path)
    parser.add_argument('--expect-unavailable',action='store_true')
    parser.add_argument('--expect-uinput-absent',action='store_true')
    args=parser.parse_args()
    evidence=dict(platform=platform.platform(),architecture=platform.machine(),qualification=False,virtualReportsSent=False,driverLayoutsChanged=False,physical='not requested',checks=[])
    if args.expect_uinput_absent:
        assert sys.platform == 'linux', 'The absent-uinput check requires Linux'
        assert not os.path.lexists('/dev/uinput'), '/dev/uinput must be absent, not just inaccessible'
        try:
            os.stat('/dev/uinput')
        except OSError as error:
            assert error.errno == errno.ENOENT, error
        else:
            raise AssertionError('/dev/uinput unexpectedly exists')
        evidence['uinputNode'] = {'path': '/dev/uinput', 'exists': False, 'errno': errno.ENOENT}
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
    if args.expect_unavailable or args.expect_uinput_absent:
        assert hello['ok'] is False, 'CI must exercise startup without an available output dependency'
    evidence['feederHello'] = hello
    evidence['optionalOutputUnavailable'] = not hello['ok']
    if hello['ok']:
        reply = json.loads(lines[1])
        assert not reply['ok'] and reply['token'] == rejected['token'] and not reply['active']
        evidence['feederEntry'] = 'installed driver; empty targets rejected before acquisition'
    else:
        evidence['feederEntry'] = 'optional dependency unavailable; process exited without activation'
    with tempfile.TemporaryDirectory(prefix='osg-profile-acceptance-') as temporary:
        directory=Path(temporary).resolve()
        obsolete=legacy_bytes(directory)
        service=Service(args.service.resolve(),directory)
        try:
            status=service.profiles('profile_status')
            default=service.profiles('profile_get',id=DEFAULT)['document']
            assert default['name']=='Default' and default['revision']==1 and not default['resources']
            assert service.request('profile_delete',id=DEFAULT,expectedRevision=1)['ok'] is False
            visibility=state(service,'controller_visibility_status','controllers')['visibility']
            assert_passive_visibility(visibility, sys.platform)
            if args.expect_unavailable and sys.platform=='win32':
                assert state(service,'controller_visibility_inspect','controllers')['visibility']['global'].get('error')
            retired_boundaries(service)
            assert not state(service,'profile_retired_recovery_status','retiredRecovery')['required']
            evidence['checks'].append('obsolete configuration ignored, old authoring/start/feedback namespaces and browser authority rejected')
            apply(service,default)
            service.profiles('profile_stop')
            document=save(service,constant_profile())
            compiled=state(service,'profile_compile','profileCompilation',document=document,assignments=[])
            assert compiled['readiness']=='ready' and compiled['programHash'],compiled
            owner=apply(service,document)
            retired_boundaries(service)
            assert service.profiles('profile_status')['profileSession']['owner']==owner
            reading=inspect(service,owner,'inverse')
            assert any(row['sample']==dict(status='known',value=dict(kind='scalar',value=False)) for row in reading['ports']),reading
            renamed=service.profiles('profile_rename',id=document['id'],expectedRevision=1,name='Saved next revision')['document']
            assert inspect(service,owner)['appliedSnapshot']['revision']==1
            assert not service.request('profile_apply',id=document['id'],expectedRevision=1)['ok']
            duplicate=service.profiles('profile_duplicate',id=document['id'],name='Copy')['document']
            assert duplicate['id']!=document['id'] and service.profiles('profile_assignments_get',id=duplicate['id'])['assignments']==[]
            stopped=service.profiles('profile_stop')
            assert stopped['profileSession']['state']=='stopped' and stopped['profileSession']['owner'] is None
            assert stopped['profileSession']['appliedSnapshot'] is None
            assert not service.request('profile_runtime_inspect',owner=owner)['ok']
            fresh=apply(service,renamed)
            assert fresh!=owner
            service.profiles('profile_stop')
            assert not service.request('profile_runtime_inspect',owner=fresh)['ok']
            assert_preserved(obsolete)
            evidence['checks'].append('actual schema2 CRUD/compile/Apply/native values, immutable applied revision, local-free Copy, Stop/restart and old-owner fences')
            if args.physical:
                vendor,product=[int(part,16) for part in args.physical.split(':')]
                def physical():
                    sources=state(service,'controller_list','controllers',offset=0)['sources']
                    return next((item for item in sources if item['vendorId']==vendor and item['productId']==product and item['online'] and item['baselineReady']),None)
                selected=wait_for(physical,'Requested physical controller has no complete baseline',8)
                controls=[]
                for offset in range(0,selected['controlCount'],32):controls.extend(state(service,'controller_inspect','controllers',sourceId=selected['id'],offset=offset)['controls'])
                assert controls
                evidence['physical']=dict(vidPid=args.physical,sourceId=selected['id'],readOnlyMetadata=True,controls=controls)
        finally:service.close()
        restarted=Service(args.service.resolve(),directory)
        try:
            assert restarted.profiles('profile_status')['profileSession']['owner'] is None
            assert restarted.profiles('profile_export',id=renamed['id'])['document']==renamed
            apply(restarted,renamed)
            restarted.profiles('profile_stop')
            assert_preserved(obsolete)
            evidence['checks'].append('fresh native restart retains portable schema2 document without resuming its old owner')
        finally:restarted.close()
    text=json.dumps(evidence,indent=2)
    print(text)
    if args.output:
        args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(text+'\n',encoding='utf-8')


if __name__=='__main__':main()
