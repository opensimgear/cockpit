#!/usr/bin/env python3
"""Independent, explicitly selected read-only hidraw evidence. Never sends a report."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import select
import stat
import struct
import sys
import time

MAX_DESCRIPTOR = 4096  # Linux hidraw_report_descriptor ABI, not the H1 parser limit.
MAX_REPORT = 65536
MAX_REPORTS = 4096
MAX_CAPTURE_BYTES = 8 * 1024 * 1024
POLLIN, POLLERR, POLLHUP, POLLNVAL = (getattr(select, name, value) for name, value in
                                    [('POLLIN', 1), ('POLLERR', 8), ('POLLHUP', 16), ('POLLNVAL', 32)])


def request(number, size):
    # asm-generic _IOR. Qualification runner intentionally restricts architectures.
    return (2 << 30) | (size << 16) | (ord('H') << 8) | number


def descriptor(fd, ioctl):
    length = bytearray(4)
    ioctl(fd, request(1, 4), length, True)
    size = struct.unpack('=I', length)[0]
    if not 1 <= size <= MAX_DESCRIPTOR:
        raise ValueError('Kernel descriptor length exceeds the hidraw ABI bound')
    data = bytearray(4 + MAX_DESCRIPTOR)
    struct.pack_into('=I', data, 0, size)
    ioctl(fd, request(2, len(data)), data, True)
    returned = struct.unpack_from('=I', data)[0]
    if returned != size:
        raise ValueError('Descriptor length changed while collecting it')
    return bytes(data[4:4 + size])


def identity(fd, ioctl):
    data = bytearray(8)
    ioctl(fd, request(3, len(data)), data, True)
    bus, vendor, product = struct.unpack('=IHH', data)
    return dict(busType=bus, vendorId=vendor, productId=product)


def capture(device, duration_ms, *, ioctl=None, opener=os.open, reader=os.read,
            closer=os.close, fstat=os.fstat, clock=time.monotonic_ns,
            poll_factory=select.poll if hasattr(select, 'poll') else None):
    if ioctl is None:
        import fcntl
        ioctl = fcntl.ioctl
    flags = os.O_RDONLY | sum(getattr(os, name, 0) for name in ('O_NONBLOCK', 'O_CLOEXEC', 'O_NOFOLLOW'))
    fd = opener(device, flags)
    try:
        opened = fstat(fd)
        if not stat.S_ISCHR(opened.st_mode):
            raise ValueError('Selected path is not a character device')
        initial_descriptor = descriptor(fd, ioctl)
        initial_identity = identity(fd, ioctl)
        poll = poll_factory()
        poll.register(fd, POLLIN | POLLERR | POLLHUP | POLLNVAL)
        began = clock()
        deadline = began + duration_ms * 1_000_000
        reports, retained = [], 0
        stopped = 'duration'
        while clock() < deadline:
            remaining = max(1, min(100, (deadline - clock() + 999_999) // 1_000_000))
            for _, events in poll.poll(remaining):
                if events & (POLLERR | POLLHUP | POLLNVAL):
                    raise OSError('Selected hidraw interface disappeared or reported an input error')
                if not events & POLLIN:
                    continue
                try:
                    raw = reader(fd, MAX_REPORT + 1)
                except BlockingIOError:
                    continue
                at = clock()  # Receipt time, before JSON conversion or queueing.
                if not raw:
                    raise OSError('Selected hidraw interface returned EOF')
                if len(raw) > MAX_REPORT:
                    raise ValueError('A hidraw report exceeds the report-byte bound')
                if len(reports) >= MAX_REPORTS or retained + len(raw) > MAX_CAPTURE_BYTES:
                    stopped = 'capacity'
                    break
                retained += len(raw)
                reports.append(dict(sequence=str(len(reports) + 1), capturedUs=str(at // 1000),
                                    wireHex=raw.hex()))
            if stopped == 'capacity':
                break
        if descriptor(fd, ioctl) != initial_descriptor or identity(fd, ioctl) != initial_identity:
            raise ValueError('Selected interface descriptor/identity changed during capture')
        return dict(origin='native-hidraw-read', descriptorSource='HIDIOCGRDESC',
                    descriptorHex=initial_descriptor.hex(),
                    descriptorSha256=hashlib.sha256(initial_descriptor).hexdigest(),
                    identity=initial_identity, reports=reports, retainedBytes=retained,
                    completed=stopped, truncated=stopped == 'capacity',
                    clock=dict(unit='microsecond', origin='native-monotonic'),
                    reportWrites=False, explicitInputReads=False, featureReads=False,
                    physicalActuationPerformed=False, fixtureEvidence=False)
    finally:
        closer(fd)


def arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--device', required=True, help='One explicitly selected /dev/hidrawN')
    parser.add_argument('--allow-hardware-read', action='store_true')
    parser.add_argument('--duration-ms', type=int, default=2000)
    args = parser.parse_args(argv)
    if not args.allow_hardware_read:
        parser.error('Real-device reads require --allow-hardware-read')
    if not re.fullmatch(r'/dev/hidraw[0-9]+', args.device):
        parser.error('Select one exact /dev/hidrawN path; no glob, symlink or automatic device choice')
    if not 100 <= args.duration_ms <= 10000:
        parser.error('duration-ms must be 100..10000')
    return args


def main(argv=None):
    args = arguments(argv)
    if sys.platform != 'linux' or platform.machine().lower() not in ('x86_64', 'aarch64'):
        raise RuntimeError('Independent probe requires Linux x86_64/aarch64 asm-generic hidraw ABI')
    result = capture(args.device, args.duration_ms)
    result['platform'] = dict(system=platform.system(), release=platform.release(),
                              architecture=platform.machine(), python=platform.python_version())
    print(json.dumps(result, separators=(',', ':')))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, RuntimeError) as error:
        print(json.dumps(dict(error=type(error).__name__, message=str(error))), file=sys.stderr)
        sys.exit(1)
