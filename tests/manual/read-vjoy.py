"""Read vJoy 3 through WinMM as an independent game consumer; never feed it."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import datetime
import json
from pathlib import Path
import time

class JoyInfoEx(c.Structure):
    _fields_ = [(name, w.DWORD) for name in (
        'size', 'flags', 'x', 'y', 'z', 'r', 'u', 'v', 'buttons',
        'buttonNumber', 'pov', 'reserved1', 'reserved2')]

assert c.sizeof(JoyInfoEx) == 52
winmm = c.WinDLL('winmm', use_last_error=True)
winmm.joyGetPosEx.argtypes = [w.UINT, c.POINTER(JoyInfoEx)]
winmm.joyGetPosEx.restype = w.UINT

def sample():
    state = JoyInfoEx()
    state.size, state.flags = c.sizeof(state), 0xff
    result = winmm.joyGetPosEx(3, c.byref(state))
    if result:
        raise RuntimeError(f'joyGetPosEx vJoy 3 failed: {result}')
    return {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'x': state.x, 'y': state.y, 'z': state.z, 'buttons': state.buttons,
            'pov1': state.pov}

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--seconds', type=float, default=0)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if not 0 <= args.seconds <= 120:
        raise ValueError('Duration must be bounded to 120 seconds')
    samples = []
    end = time.monotonic() + args.seconds
    while True:
        samples.append(sample())
        if time.monotonic() >= end:
            break
        time.sleep(.1)
    report = {'scope': 'Independent WinMM consumer read of verified vJoy 3 joystick ID',
              'feederAcquisitions': 0, 'setOperations': 0, 'samples': samples}
    if args.output:
        args.output.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps({'count': len(samples), 'first': samples[0], 'last': samples[-1],
                      'ranges': {key: [min(s[key] for s in samples), max(s[key] for s in samples)]
                                 for key in ('x', 'y', 'z', 'buttons', 'pov1')}}, indent=2))
