"""Compare authored callback bytes with the pinned installed SDK's pure helpers.

No device, ownership, callback registration, configuration or output API is called.
ABI and field offsets: BrunnerInnovation/vJoy commit
bd268d1fa5934a900a79e76e519d4aa804e0ee3d, SDK/inc/vjoyinterface.h and
apps/common/vJoyInterface/vJoyInterface.cpp (Ffb_h_* implementations).
https://github.com/BrunnerInnovation/vJoy/tree/bd268d1fa5934a900a79e76e519d4aa804e0ee3d
"""
import argparse
import ctypes as c
import hashlib
import json
from pathlib import Path
import struct

DLL_SHA256 = '199316aaefac95118e869c07e7f332ce492dd9a8b6dac5a3ca20b1d5bb5058a1'
WRITE_REPORT, SET_FEATURE = 0xB0017, 0xB0191
U8, U16, U32, I32 = c.c_uint8, c.c_uint16, c.c_uint32, c.c_int32


class Packet(c.Structure):
    _fields_ = [('size', U32), ('cmd', U32), ('data', c.POINTER(U8))]


class Effect(c.Structure):
    _fields_ = [('block', U8), ('type', U32), ('duration', U16),
                ('trigger', U16), ('sample', U16), ('delay', U16),
                ('gain', U8), ('button', U8), ('axes', U8), ('polar', I32),
                ('x', U16), ('y', U16)]


class Operation(c.Structure):
    _fields_ = [('block', U8), ('operation', U32), ('loops', U8)]


class Constant(c.Structure):
    _fields_ = [('block', U8), ('magnitude', I32)]


class Ramp(c.Structure):
    _fields_ = [('block', U8), ('start', I32), ('end', I32)]


class Period(c.Structure):
    _fields_ = [('block', U8), ('magnitude', U32), ('offset', I32),
                ('phase', U32), ('period', U32)]


class Condition(c.Structure):
    _fields_ = [('block', U8), ('is_y', I32), ('center', I32), ('positive', I32),
                ('negative', I32), ('pos_sat', U32), ('neg_sat', U32), ('dead', I32)]


class Envelope(c.Structure):
    _fields_ = [('block', U8), ('attack', U32), ('fade', U32),
                ('attack_time', U32), ('fade_time', U32)]


TYPES = {1:'setEffect', 2:'setEnvelope', 3:'setCondition', 4:'setPeriodic',
         5:'setConstant', 6:'setRamp', 7:'setCustomForce', 8:'downloadForceSample',
         10:'effectOperation', 11:'blockFree', 12:'deviceControl', 13:'deviceGain',
         14:'setCustomForce', 17:'createNewEffect', 18:'blockLoad', 19:'pool'}
EFFECTS = {1:'constant', 2:'ramp', 3:'square', 4:'sine', 5:'triangle',
           6:'sawtoothUp', 7:'sawtoothDown', 8:'spring', 9:'damper',
           10:'inertia', 11:'friction', 12:'custom'}
OPERATIONS = {1:'start', 2:'solo', 3:'stop'}
CONTROLS = {1:'enableActuators', 2:'disableActuators', 3:'stopAll',
            4:'deviceReset', 5:'pause', 6:'continue'}


def signed16(value):
    # The pinned helper zero-extends WORD expressions into LONG. The wire
    # descriptor declares these fields signed; compare the signed projection.
    return c.c_int16(value & 0xffff).value


def milliseconds(value):
    return 'infinite' if value == 65535 else str(value)


class Sdk:
    def __init__(self, path):
        assert c.sizeof(c.c_void_p) == 8, 'Use Windows x64 Python'
        assert hashlib.sha256(path.read_bytes()).hexdigest() == DLL_SHA256, 'Unqualified SDK DLL'
        assert c.sizeof(Packet) == 16 and Packet.data.offset == 8
        assert (c.sizeof(Effect), Effect.polar.offset, Effect.x.offset) == (28,20,24)
        assert [c.sizeof(t) for t in (Operation,Constant,Ramp,Period,Condition,Envelope)] == [12,8,12,20,32,20]
        self.dll = c.CDLL(str(path), winmode=0x1100)

    def helper(self, name, packet, kind):
        # Only helper exports are resolved, so this verifier cannot feed a slot.
        assert name.startswith('Ffb_h_')
        function = getattr(self.dll, name)
        function.argtypes = [c.POINTER(Packet), c.POINTER(kind)]
        function.restype = U32
        value = kind()
        result = function(c.byref(packet), c.byref(value))
        assert result == 0, f'{name}: SDK returned {result}'
        return value

    def decode(self, raw, command):
        # Ffb_h_EffectBlockIndex accepts size>=10 but reads data[2] for a new
        # effect; other helpers rely on similarly coarse hardcoded minima.
        # We never use their return codes to validate malformed callback bytes.
        # Allocate a zero-filled local guard region, while retaining the exact
        # original byte count in FFB_DATA.size. Padding is never fixture data.
        storage = (U8 * 256)()
        assert len(raw) <= 248
        storage[:len(raw)] = raw
        packet = Packet(len(raw)+8, command, storage)
        device = self.helper('Ffb_h_DeviceID', packet, U32).value
        kind = self.helper('Ffb_h_Type', packet, U32).value
        result = {'slot':device, 'reportName':TYPES.get(kind, 'unknown')}
        fields = {}
        if kind == 1:
            value = self.helper('Ffb_h_Eff_Report', packet, Effect)
            fields = dict(effectBlockIndex=value.block, effectType=EFFECTS.get(value.type,'unknown'),
                          durationMs=milliseconds(value.duration), triggerRepeatMs=milliseconds(value.trigger),
                          samplePeriodMs=milliseconds(value.sample), startDelayMs=value.delay,
                          gainRaw=value.gain, triggerButton=value.button, axesEnable=value.axes,
                          direction={'mode':'polar','raw':value.x} if value.polar else
                          {'mode':'cartesian','x':signed16(value.x),'y':signed16(value.y)})
        elif kind == 2:
            value = self.helper('Ffb_h_Eff_Envlp', packet, Envelope)
            fields = dict(effectBlockIndex=value.block, attackLevel=value.attack,
                          fadeLevel=value.fade, attackTimeMs=value.attack_time, fadeTimeMs=value.fade_time)
        elif kind == 3:
            value = self.helper('Ffb_h_Eff_Cond', packet, Condition)
            fields = dict(effectBlockIndex=value.block, isY=bool(value.is_y),
                          centerPointOffset=signed16(value.center), positiveCoefficient=signed16(value.positive),
                          negativeCoefficient=signed16(value.negative), positiveSaturation=value.pos_sat,
                          negativeSaturation=value.neg_sat, deadBand=signed16(value.dead))
        elif kind == 4:
            value = self.helper('Ffb_h_Eff_Period', packet, Period)
            fields = dict(effectBlockIndex=value.block, magnitude=value.magnitude,
                          offset=signed16(value.offset), phase=value.phase, periodMs=value.period)
        elif kind == 5:
            value = self.helper('Ffb_h_Eff_Constant', packet, Constant)
            fields = dict(effectBlockIndex=value.block, magnitude=signed16(value.magnitude))
        elif kind == 6:
            value = self.helper('Ffb_h_Eff_Ramp', packet, Ramp)
            fields = dict(effectBlockIndex=value.block, start=signed16(value.start), end=signed16(value.end))
        elif kind == 10:
            value = self.helper('Ffb_h_EffOp', packet, Operation)
            fields = dict(effectBlockIndex=value.block, operation=OPERATIONS.get(value.operation,'unknown'),
                          operationRaw=value.operation, loopCount=value.loops)
        elif kind == 12:
            value = self.helper('Ffb_h_DevCtrl', packet, U32).value
            fields = dict(control=CONTROLS.get(value,'unknown'), controlRaw=value)
        elif kind == 13:
            fields = dict(gainRaw=self.helper('Ffb_h_DevGain', packet, U8).value)
        elif kind == 17:
            function = self.dll.Ffb_h_CreateNewEffect
            function.argtypes = [c.POINTER(Packet),c.POINTER(U32),c.POINTER(U32)]
            function.restype = U32
            effect, block = U32(), U32()
            assert function(c.byref(packet),c.byref(effect),c.byref(block)) == 0
            fields = dict(effectType=EFFECTS.get(effect.value,'unknown'), effectTypeRaw=effect.value,
                          effectBlockIndex=block.value)
        result['fields'] = fields
        return result


def golden_vectors():
    # Independent authored, nonzero boundary values make field swaps observable.
    # Report ID high nibble is the device, low nibble is report kind. Feature
    # kind is distinguished by command, not by making a different report byte.
    return [
        ('create', SET_FEATURE, bytes.fromhex('11040300'), dict(effectType='sine',effectTypeRaw=4,effectBlockIndex=3)),
        ('set-effect-cartesian', WRITE_REPORT,
         b'\x11\x03\x08'+struct.pack('<HHHHBBBhh',65535,234,345,456,199,12,3,-1234,2345)+b'\0\0',
         dict(effectBlockIndex=3,effectType='spring',durationMs='infinite',triggerRepeatMs='234',
              samplePeriodMs='345',startDelayMs=456,gainRaw=199,triggerButton=12,axesEnable=3,
              direction={'mode':'cartesian','x':-1234,'y':2345})),
        ('set-effect-polar', WRITE_REPORT,
         b'\x11\x03\x04'+struct.pack('<HHHHBBBH',1234,65535,345,456,199,12,4,12345)+b'\0\0',
         dict(effectBlockIndex=3,effectType='sine',durationMs='1234',triggerRepeatMs='infinite',
              samplePeriodMs='345',startDelayMs=456,gainRaw=199,triggerButton=12,axesEnable=4,
              direction={'mode':'polar','raw':12345})),
        ('constant-negative', WRITE_REPORT, bytes.fromhex('15032efb'), dict(effectBlockIndex=3,magnitude=-1234)),
        ('operation', WRITE_REPORT, bytes.fromhex('1a030207'), dict(effectBlockIndex=3,operation='solo',operationRaw=2,loopCount=7)),
        ('ramp', WRITE_REPORT, bytes.fromhex('160318fc2923'), dict(effectBlockIndex=3,start=-1000,end=9001)),
        ('envelope', WRITE_REPORT, b'\x12\x03'+struct.pack('<HHII',257,8193,65537,0x12345678),
         dict(effectBlockIndex=3,attackLevel=257,fadeLevel=8193,attackTimeMs=65537,fadeTimeMs=0x12345678)),
        ('condition', WRITE_REPORT, b'\x13\x03\x01'+struct.pack('<hhhHHh',-1234,2345,-3456,4567,5678,789),
         dict(effectBlockIndex=3,isY=True,centerPointOffset=-1234,positiveCoefficient=2345,
              negativeCoefficient=-3456,positiveSaturation=4567,negativeSaturation=5678,deadBand=789)),
        ('period', WRITE_REPORT, b'\x14\x03'+struct.pack('<HhHI',6001,-2001,12345,0x12345678),
         dict(effectBlockIndex=3,magnitude=6001,offset=-2001,phase=12345,periodMs=0x12345678)),
        ('gain', WRITE_REPORT, bytes.fromhex('1da5'), dict(gainRaw=165)),
        ('control', WRITE_REPORT, bytes.fromhex('1c05'), dict(control='pause',controlRaw=5)),
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dll',type=Path,default=Path(r'C:\Program Files\vJoy\x64\vJoyInterface.dll'))
    parser.add_argument('--fixture',type=Path,default=Path(__file__).with_name('effects-lifecycle.json'))
    args = parser.parse_args()
    sdk = Sdk(args.dll)
    checks = []
    for name, command, raw, expected in golden_vectors():
        decoded = sdk.decode(raw,command)
        assert decoded['slot'] == 1, (name,decoded)
        assert decoded['fields'] == expected, (name,decoded['fields'],expected)
        checks.append({'name':name,'capturedBytes':len(raw),'sdk':decoded})
    device_boundary = sdk.decode(bytes.fromhex('fd80'),WRITE_REPORT)
    assert device_boundary == {'slot':15,'reportName':'deviceGain','fields':{'gainRaw':128}}
    checks.append({'name':'device-15-type-gain','capturedBytes':2,'sdk':device_boundary})
    fixture = json.loads(args.fixture.read_text(encoding='utf-8-sig'))
    for packet in fixture['packets']:
        if packet['expect']['status'] != 'ok':
            continue  # The helper's weak bounds checks do not qualify malformed capture safety.
        raw = bytes.fromhex(packet['bytesHex'])
        decoded = sdk.decode(raw,packet.get('command',fixture.get('command',WRITE_REPORT)))
        assert decoded['slot'] == packet.get('slot',fixture['slot']), (packet['name'],decoded)
        expected = packet['expect']
        assert decoded['reportName'] == expected['reportName'], (packet['name'],decoded,expected)
        for field in ['effectType','control']:
            if field in expected:
                assert decoded['fields'][field] == expected[field], (packet['name'],field,decoded,expected)
        for field, value in expected.get('fields',{}).items():
            if field in decoded['fields']:
                assert decoded['fields'][field] == value, (packet['name'],field,decoded,value)
        # The final set-effect state supplies independent native decoder golden fields.
        if decoded['reportName'] == 'setEffect':
            final = next(row['fields'] for row in fixture['expectedSnapshot']['effects']
                         if row['blockIndex'] == decoded['fields']['effectBlockIndex'])
            for field, value in decoded['fields'].items():
                assert final[field] == value, (packet['name'],field,final[field],value)
        checks.append({'name':packet['name'],'capturedBytes':len(raw),'sdk':decoded})
    print(json.dumps({'dllSha256':DLL_SHA256,'checks':checks,'count':len(checks),
                      'projection':'WORD expressions stored in SDK LONG are interpreted as signed 16-bit descriptor values; original packet.size is retained despite guarded backing allocation',
                      'scope':'pure decoder helpers only; zero device/output/configuration calls'},indent=2))


if __name__ == '__main__':
    main()
