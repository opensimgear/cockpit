"""Portable guards for the opt-in desktop worker-loss fixture."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import Mock, patch


spec = importlib.util.spec_from_file_location("worker_loss", Path(__file__).with_name("desktop-worker-loss-native.py"))
worker_loss = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker_loss)


class WorkerLossGuards(unittest.TestCase):
    def interrupt(self, parent, expected):
        app = dict(pid=123, created="100", executable="cockpit.exe")
        worker = dict(pid=124, created="200", executable="cockpit.exe")
        identity = dict(pid=125, created="300", executable="receiver.exe")
        target = dict(window="54321", identity=identity)
        windows = Mock()
        windows.identity.return_value = worker
        windows.path.side_effect = lambda value: value
        windows.open_exact.return_value = 456
        windows.parents.return_value = {124: parent}
        windows.exact.return_value = True
        windows.foreground.return_value = dict(window=54321, identity=identity)
        windows.kernel.WaitForSingleObject.side_effect = [258, 0]
        with patch.object(worker_loss, "workers", return_value=[{"pid": 124}]):
            if expected:
                actual, requested, exited = worker_loss.interrupt_worker(object(), windows, app, target)
                self.assertEqual(actual, worker)
                self.assertGreaterEqual(exited, requested)
                windows.kernel.TerminateProcess.assert_called_once_with(456, 97)
            else:
                with self.assertRaises(AssertionError):
                    worker_loss.interrupt_worker(object(), windows, app, target)
                windows.kernel.TerminateProcess.assert_not_called()
        windows.kernel.CloseHandle.assert_called_once_with(456)

    def test_terminates_only_the_exact_private_child(self):
        self.interrupt(123, True)

    def test_refuses_a_worker_with_foreign_ancestry(self):
        self.interrupt(456, False)

    def test_uncertain_journal_is_not_cleanup_proof(self):
        payload = {"held": [], "possibleHeld": [], "owners": [], "runs": [],
            "cleanupComplete": True, "uncertain": True, "inFlight": None}
        with self.assertRaises(AssertionError):
            worker_loss.journal_clean({"payload": payload})
        worker_loss.journal_clean({"payload": {**payload, "uncertain": False}})


if __name__ == "__main__":
    unittest.main()
