"""Offline fake-kernel tests. No /dev node, ioctl, browser or Cockpit is accessed."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import stat
import struct
from types import SimpleNamespace
import unittest

source = Path(__file__).with_name('verify-linux-probe.py')
spec = importlib.util.spec_from_file_location('linux_probe', source)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class Kernel:
    def __init__(self, mode='normal', report=b'\x04\x2a', tick=1_000_000):
        self.mode, self.report, self.tick = mode, report, tick
        self.calls, self.now, self.descriptors = [], 0, 0

    def ioctl(self, fd, request, data, mutate):
        self.calls.append(('ioctl', request & 255))
        assert fd == 123 and mutate is True
        if request & 255 == 1:
            struct.pack_into('=I', data, 0, 4097 if self.mode == 'over-descriptor' else 2)
        elif request & 255 == 2:
            self.descriptors += 1
            if self.mode == 'descriptor-race':
                struct.pack_into('=I', data, 0, 3)
            data[4:6] = b'\x05\x01' if self.descriptors == 1 or self.mode != 'changed' else b'\x05\x02'
        elif request & 255 == 3:
            struct.pack_into('=IHH', data, 0, 3, 0x8001, 0xffff)
        else:
            raise AssertionError('Unexpected ioctl: no feature/input/output request allowed')

    def open(self, device, flags):
        self.calls.append(('open', device, flags))
        assert device == '/dev/hidraw7'
        assert flags & (os.O_WRONLY | os.O_RDWR) == 0
        return 123

    def close(self, fd):
        self.calls.append(('close', fd))

    def fstat(self, fd):
        return SimpleNamespace(st_mode=stat.S_IFREG if self.mode == 'regular' else stat.S_IFCHR)

    def clock(self):
        self.now += self.tick
        return self.now

    def poll(self, timeout):
        return [(123, probe.POLLHUP if self.mode == 'removed' else probe.POLLIN)]

    def register(self, fd, flags):
        assert fd == 123 and flags == probe.POLLIN | probe.POLLERR | probe.POLLHUP | probe.POLLNVAL

    def read(self, fd, maximum):
        assert maximum == probe.MAX_REPORT + 1
        if self.mode == 'would-block':
            raise BlockingIOError()
        return self.report

    def capture(self):
        return probe.capture('/dev/hidraw7', 30, ioctl=self.ioctl, opener=self.open,
                             reader=self.read, closer=self.close, fstat=self.fstat,
                             clock=self.clock, poll_factory=lambda: self)


class ProbeTests(unittest.TestCase):
    def test_exact_original_bytes_identity_clock_and_metadata_only_ioctls(self):
        kernel = Kernel()
        result = kernel.capture()
        self.assertEqual(result['descriptorHex'], '0501')
        self.assertEqual(result['identity'], dict(busType=3, vendorId=0x8001, productId=0xffff))
        self.assertTrue(result['reports'])
        self.assertTrue(all(row['wireHex'] == '042a' for row in result['reports']))
        self.assertEqual(result['reports'][0]['sequence'], '1')
        self.assertEqual(result['retainedBytes'], len(result['reports']) * 2)
        self.assertFalse(result['truncated'])
        self.assertFalse(result['fixtureEvidence'])  # Test result is not persisted as hardware evidence.
        self.assertEqual({call[1] for call in kernel.calls if call[0] == 'ioctl'}, {1, 2, 3})
        self.assertEqual(kernel.calls[-1], ('close', 123))

    def test_independent_descriptor_abi_errors_and_replacement_are_not_accepted(self):
        for mode in ['over-descriptor', 'descriptor-race', 'changed', 'regular', 'removed']:
            with self.subTest(mode=mode):
                kernel = Kernel(mode)
                with self.assertRaises((ValueError, OSError)):
                    kernel.capture()
                self.assertEqual(kernel.calls[-1], ('close', 123))

    def test_eof_and_over_report_close_the_reader(self):
        for report in [b'', b'\0' * (probe.MAX_REPORT + 1)]:
            kernel = Kernel(report=report)
            with self.assertRaises((OSError, ValueError)):
                kernel.capture()
            self.assertEqual(kernel.calls[-1], ('close', 123))

    def test_no_reports_is_explicit_not_synthetic_neutral(self):
        result = Kernel('would-block').capture()
        self.assertEqual(result['reports'], [])
        self.assertEqual(result['retainedBytes'], 0)

    def test_record_and_byte_limits_seal_a_truthful_prefix(self):
        kernel = Kernel(report=b'\1', tick=1)
        result = kernel.capture()
        self.assertEqual(len(result['reports']), probe.MAX_REPORTS)
        self.assertEqual(result['completed'], 'capacity')
        self.assertTrue(result['truncated'])
        result = Kernel(report=b'\1' * probe.MAX_REPORT, tick=1).capture()
        self.assertEqual(result['retainedBytes'], probe.MAX_CAPTURE_BYTES)
        self.assertTrue(result['truncated'])

    def test_cli_requires_one_explicit_device_and_hardware_opt_in(self):
        for arguments in [[], ['--device', '/dev/hidraw7'],
                          ['--device', '/dev/hidraw*', '--allow-hardware-read'],
                          ['--device', '/dev/input/event7', '--allow-hardware-read'],
                          ['--device', '/dev/hidraw7', '--allow-hardware-read', '--duration-ms', '0']]:
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                probe.arguments(arguments)
        accepted = probe.arguments(['--device', '/dev/hidraw7', '--allow-hardware-read'])
        self.assertEqual(accepted.device, '/dev/hidraw7')


if __name__ == '__main__':
    unittest.main(verbosity=2)
