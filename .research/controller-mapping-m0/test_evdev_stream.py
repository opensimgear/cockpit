import unittest
from evdev_stream import EvdevStream


class EvdevTests(unittest.TestCase):
    def test_events_are_committed_at_syn_report(self):
        stream = EvdevStream()
        stream.baseline({(1, 288): 0, (3, 0): 0})
        self.assertIsNone(stream.event(1, 288, 1))
        self.assertIsNone(stream.event(3, 0, 50))
        self.assertEqual(stream.current[(1, 288)], 0)
        self.assertEqual(stream.event(0, 0, 0), (1, {(1, 288): 1, (3, 0): 50}))

    def test_dropped_reports_release_and_require_new_baseline(self):
        stream = EvdevStream()
        stream.baseline({(1, 288): 0})
        stream.event(1, 288, 1)
        self.assertEqual(stream.event(0, 3, 0), "release-and-rebaseline")
        self.assertIsNone(stream.event(1, 288, 0))
        self.assertIsNone(stream.event(0, 0, 0))
        self.assertEqual(stream.current, {})
        self.assertEqual(stream.baseline({(1, 288): 0}), (2, {(1, 288): 0}))

    def test_unknown_controls_cannot_change_capabilities(self):
        stream = EvdevStream()
        stream.baseline({(1, 288): 0})
        with self.assertRaises(ValueError):
            stream.event(1, 289, 1)
        self.assertEqual(stream.current, {(1, 288): 0})


if __name__ == "__main__":
    unittest.main()
