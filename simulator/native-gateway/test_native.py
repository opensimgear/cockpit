#!/usr/bin/env python3
"""Behavioral acceptance through production CDC -> Cyphal -> module handlers."""
import argparse
import json
from pathlib import Path
import select
import subprocess
import tempfile
import os

MODULE='00112233445566778899aabbccddeeff'

def report_bytes(descriptor):
    """Read HID short items; derive report lengths independently of firmware."""
    raw=bytes.fromhex(descriptor); i=0; size=count=report=0; bits={}
    while i<len(raw):
        prefix=raw[i];i+=1; n=prefix&3; n=4 if n==3 else n
        assert prefix!=254 and i+n<=len(raw)
        value=int.from_bytes(raw[i:i+n],'little'); i+=n
        if prefix==0x75:size=value
        elif prefix==0x95:count=value
        elif prefix==0x85:report=value
        elif (prefix&0xfc)==0x80:bits[report]=bits.get(report,0)+size*count
    return {r:(n+7)//8 for r,n in bits.items()}

class Device:
    def __init__(self,binary,state=None):
        environment=os.environ.copy()
        environment.pop('OSG_NATIVE_INITIAL_SETTINGS_FAULT',None)
        if state:environment['OSG_NATIVE_STATE_DIR']=str(state)
        else:environment.pop('OSG_NATIVE_STATE_DIR',None)
        self.process=subprocess.Popen([str(binary)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=environment)
        self.sequence=0; self.epoch=65536; self.boot=1
    def raw(self,line):
        self.process.stdin.write(line+'\n');self.process.stdin.flush()
        assert select.select([self.process.stdout],[],[],10)[0],f'No response to {line}'
        output=self.process.stdout.readline();assert output,f'Emulator exited on {line}'
        if output.startswith('{'):return json.loads(output)
        return output.strip()
    def request(self,operation,expected=None,identity=MODULE,epoch=None,boot=None,id=None):
        self.sequence+=1; id=self.sequence if id is None else id
        command=f'cockpit {id} {operation}' if operation=='snapshot' else f'cockpit {id} {self.epoch if epoch is None else epoch} {operation}'
        result=self.raw(command)
        assert result['id']==id
        assert result['error']==expected,(operation,result)
        if result.get('state'):
            self.epoch=result['state']['epoch']
            for module in result['state']['modules']:
                if module['identity']==MODULE and module.get('boot'):self.boot=module['boot']
        return result
    def config(self,child,expected=None,**kw):
        boot=kw.pop('boot',self.boot)
        return self.request(f'config {MODULE} {boot} {child}',expected,**kw)
    def admin(self,child):
        self.sequence+=1; result=self.raw(f'emulator {self.sequence} {child}');assert result['ok'],result
        self.epoch=result['emulator']['gatewayEpoch'];self.boot=result['emulator']['moduleBoot'];return result['emulator']
    def close(self):
        self.process.stdin.close();assert self.process.wait(timeout=10)==0

def acceptance(binary):
    d=Device(binary)
    try:
        assert 'identity=00112233445566778899aabb' in d.raw('identity')
        initial=d.request('snapshot')['state']; assert len(initial['modules'])==1
        defaults=d.config('get')['targetConfig'];assert defaults['layout']['reportBytes']==4
        for field in ('source','boot','lease','epoch','node','identity','reordered'):
            assert d.admin('probe_config '+field)['module']['owner']==0
        d.request('register '+MODULE); d.request('apply')
        before=d.admin('inspect');assert before['gatewayReconnects']==1
        assert report_bytes(before['descriptorHex'])=={1:4}
        d.admin('capture_input');d.admin('capture_delivery')
        d.config('claim 99');d.config('claim 100','writer_busy')
        d.config('counts 99 4 2 1 1 1 0')
        d.config('button 99 3 11 0 0')
        d.config('axis 99 1 1 49 100 3900 32768 1 800')
        name='Drive Rig ✓'
        d.config('name 99 '+name.encode().hex())
        staged=d.config('get')['targetConfig'];assert staged['valid'] and staged['dirty']
        assert staged['layout']=={'buttons':9,'axes':2,'hats':1,'reportBytes':7}
        assert d.admin('inspect')['descriptorHex']==before['descriptorHex']
        d.config('axis 99 1 1 49 3000 100 32768 1 800','invalid_axis')
        d.config('button 99 3 0 0 0'); d.config('apply 99','invalid_profile')
        assert d.admin('inspect')['moduleReconnects']==0
        d.config('button 99 3 11 0 0');d.config('apply_save 99')
        configured=d.config('get_active')['targetConfig']
        assert configured['config']['name']==name and configured['generation']==configured['savedGeneration']==2
        assert not configured['pendingApply'] and report_bytes(configured['descriptorHex'])=={0:7}
        middle=d.admin('inspect');assert middle['moduleReconnects']==1
        assert middle['descriptorHex']==before['descriptorHex']
        snapshot=d.request('snapshot')['state'];assert snapshot['needsApply'] and not snapshot['modules'][0]['profileMatch']
        d.admin('fault gateway 9');d.request('apply','flash_save_failed')
        assert d.admin('inspect')['descriptorHex']==before['descriptorHex']
        d.request('apply');after=d.admin('inspect')
        assert after['gatewayReconnects']==2 and report_bytes(after['descriptorHex'])=={1:7}
        assert d.admin('stale_input')['staleRejected']==1
        assert d.admin('complete_delivery')['staleRejected']==2
        d.admin('axis 0 0');d.admin('axis 1 100');low=d.admin('inspect')['gatewayReports'][0]['hex']
        assert bytes.fromhex(low)[-4:]==bytes([0,0,255,255])
        d.admin('axis 0 4095');d.admin('axis 1 3900');high=d.admin('inspect')['gatewayReports'][0]['hex']
        assert bytes.fromhex(high)[-4:]==bytes([255,255,0,0])
        d.admin('contact 11 1');assert bytes.fromhex(d.admin('inspect')['gatewayReports'][0]['hex'])[0]&8
        d.admin('direct 1');direct=d.request('snapshot')['state']['modules'][0]
        assert direct['directUsb'] and not direct['hasInput']
        assert bytes.fromhex(direct['report'])[-4:]==bytes([0,128,0,128])
        d.admin('direct 0');d.admin('disconnect 1');assert not d.request('snapshot')['state']['running'];d.admin('disconnect 0')
        d.config('claim 99');d.config('name 99 '+b'Volatile'.hex());d.admin('capture_input')
        prior=d.request('snapshot')['state']['modules'][0]['lease'];d.config('apply 99')
        same_shape=d.request('snapshot')['state'];assert not same_shape['needsApply'] and same_shape['modules'][0]['lease']!=prior
        assert d.admin('stale_input')['staleRejected']==3
        assert d.config('get_active')['targetConfig']['config']['name']=='Volatile'
        old_boot=d.boot;d.admin('reboot module');d.config('get',boot=old_boot,expected='stale_module_boot')
        restored=d.config('get')['targetConfig'];assert restored['config']['name']==name and restored['generation']==2
        d.config('claim 99');d.config('name 99 '+b'Failed write'.hex());d.admin('fault module 9')
        d.config('apply_save 99','flash_save_failed');assert d.admin('inspect')['module']['generation']==2
        d.admin('reboot module');assert d.config('get')['targetConfig']['config']['name']==name
        d.config('claim 99');saved_before=d.admin('inspect')['settingsPrograms']
        request_id=5000;d.config('save 99',id=request_id);saved=d.admin('inspect')['settingsPrograms'];assert saved>saved_before
        d.config('save 99',id=request_id);assert d.admin('inspect')['settingsPrograms']==saved
        # Losing all reply fragments yields bounded timeout and next command recovers.
        d.admin('drop_reply 1');d.config('get','configuration_timeout');d.admin('drop_reply 0');d.config('get')
        d.admin('advance 30001');assert d.config('get')['targetConfig']['owner']==0
        external_before=d.request('snapshot')['state']['modules'][0]['configurationGeneration']
        d.admin('direct_config claim 44');d.admin('direct_config name 44 '+b'External direct writer'.hex())
        d.admin('direct_config apply_save 44')
        external=d.request('snapshot')['state'];assert not external['needsApply']
        assert external['modules'][0]['configurationGeneration']==external_before+1
        assert d.config('get_active')['targetConfig']['config']['name']=='External direct writer'
        d.admin('direct_config release 44')
        d.config('claim 77');d.config('name 77 '+b'Draft lost with gateway lease'.hex())
        old_epoch=d.epoch;d.admin('reboot gateway');d.request('save',expected='stale_epoch',epoch=old_epoch)
        assert d.config('get')['targetConfig']['owner']==0
        state=d.request('snapshot')['state'];assert not state['needsApply'] and state['modules'][0]['activeSlot']==0
        assert d.admin('inspect')['acks']>0
    finally:d.close()
    print('PASS: native CDC/CAN configuration, ownership, descriptor activation, calibrated inputs, reboot, interrupted persistence and replay')

def maximum_controls(binary):
    d=Device(binary)
    try:
        d.request('register '+MODULE);d.request('apply');d.config('claim 19')
        d.config('counts 19 128 8 4 0 0 255');d.config('name 19 '+('M'*63).encode().hex())
        for button in range(128):d.config(f'button 19 {button} {button} 1 0')
        for axis in range(8):d.config(f'axis 19 {axis} {axis} {48+axis} 100 3900 32768 {axis%2} 750')
        for hat in range(4):d.config(f'hat 19 {hat} '+' '.join(str(128+hat*4+i) for i in range(4)))
        maximum=d.config('get')['targetConfig'];assert maximum['valid']
        assert maximum['layout']=={'buttons':128,'axes':8,'hats':4,'reportBytes':34}
        assert len(json.dumps(maximum,separators=(',',':')).encode())<4093
        d.config('apply_save 19');d.request('apply');inspect=d.admin('inspect')
        assert report_bytes(inspect['descriptorHex'])=={1:34}
        assert len(bytes.fromhex(inspect['gatewayReports'][0]['hex']))==34
        d.admin('reboot module');assert d.config('get')['targetConfig']['layout']['reportBytes']==34
    finally:d.close()
    print('PASS: full 128-button/8-axis/4-hat configuration, 63-byte name, multi-chunk replies and 34-byte HID reports')

def first_save_interruption(binary):
    # Erase, initial payload, middle payload and commit marker boundaries.
    for cut in (0,1,9,79):
        d=Device(binary)
        try:
            initial=d.config('get')['targetConfig'];assert initial['savedGeneration']==1
            d.config('claim 31');d.config('name 31 '+b'First user save'.hex())
            d.admin(f'fault module {cut}');d.config('apply_save 31','flash_save_failed')
            d.admin('reboot module');restored=d.config('get')['targetConfig']
            assert restored['config']['name']=='example' and restored['generation']==restored['savedGeneration']==1
        finally:d.close()
    print('PASS: interrupted first user saves preserve the committed factory baseline through reboot')

def persistent_process(binary,workspace):
    with tempfile.TemporaryDirectory(dir=workspace/'.research',prefix='native-emulator-persistence-') as path:
        d=Device(binary,Path(path))
        d.request('register '+MODULE);d.request('apply');d.config('claim 11');d.config('name 11 '+b'Persisted between processes'.hex());d.config('apply_save 11');d.close()
        d=Device(binary,Path(path));d.raw('identity');d.request('snapshot');state=d.config('get')['targetConfig']
        assert state['config']['name']=='Persisted between processes' and state['savedGeneration']==2
        assert d.epoch==131072 and d.boot==2;d.close()
    print('PASS: persisted settings, registry and monotonic boot/epoch survive process restart')

def interrupted_factory_baseline(binary,workspace):
    for cut in (0,1,9,79):
        with tempfile.TemporaryDirectory(dir=workspace/'.research',prefix='native-baseline-recovery-') as path:
            environment=os.environ.copy();environment['OSG_NATIVE_STATE_DIR']=path
            environment['OSG_NATIVE_INITIAL_SETTINGS_FAULT']=str(cut)
            interrupted=subprocess.run([str(binary)],input='identity\n',text=True,capture_output=True,env=environment)
            assert interrupted.returncode!=0,'An interrupted initial factory commit must not serve inputs'
            d=Device(binary,Path(path));d.request('snapshot')
            restored=d.config('get')['targetConfig'];assert restored['generation']==restored['savedGeneration']==1
            assert restored['config']['name']=='example';d.close()
    print('PASS: interrupted initial factory baseline restores only deterministic baseline journal bytes')

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',type=Path)
    args=parser.parse_args();workspace=Path(__file__).resolve().parents[3]
    binary=(args.binary or workspace/'.research/native-gateway/native-gateway').resolve()
    acceptance(binary);maximum_controls(binary);first_save_interruption(binary);persistent_process(binary,workspace)
    interrupted_factory_baseline(binary,workspace)

if __name__=='__main__':main()
