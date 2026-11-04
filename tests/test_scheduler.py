import random
import tempfile
import unittest
from pathlib import Path

from dede_tv.scheduler import Video, VirtualChannels, scan_media


class SchedulerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dbpath = Path(self.tmp.name) / "state.db"
        self.channels = VirtualChannels(self.dbpath, rng=random.Random(3))

    def tearDown(self):
        self.channels.close()
        self.tmp.cleanup()

    def test_time_continues_while_not_viewed(self):
        self.channels.set_channel("Film", [Video("one.mp4", 1000)], now_ms=100)
        self.assertEqual(self.channels.current("Film", now_ms=350).position_ms, 250)
        self.assertEqual(self.channels.current("Film", now_ms=900).position_ms, 800)

    def test_restart_persists(self):
        self.channels.set_channel("Film", [Video("one.mp4", 1000)], now_ms=100)
        self.channels.close()
        self.channels = VirtualChannels(self.dbpath, rng=random.Random(3))
        self.assertEqual(self.channels.current("Film", now_ms=1100).position_ms, 0)

    def test_no_repeat_at_shuffled_cycle_boundary(self):
        self.channels.set_channel("Film", [
            Video("one", 100), Video("two", 100), Video("three", 100)
        ], now_ms=0)
        self.assertNotEqual(
            self.channels.current("Film", now_ms=299).path,
            self.channels.current("Film", now_ms=300).path
        )

    def test_invalid_playlist(self):
        with self.assertRaises(ValueError):
            self.channels.set_channel("Film", [], now_ms=0)
        with self.assertRaises(ValueError):
            self.channels.set_channel("Film", [Video("bad", -1)], now_ms=0)

    def test_scan_ignores_symlink(self):
        root = Path(self.tmp.name)
        (root / "a.mkv").touch()
        (root / "b.txt").touch()
        (root / "evil.mp4").symlink_to(root / "a.mkv")
        self.assertEqual(
            [v.path for v in scan_media(root, lambda p: 2000)],
            [str(root / "a.mkv")]
        )


if __name__ == "__main__":
    unittest.main()
