import sys
import threading
import unittest

from metrics import Metrics


class MetricsTest(unittest.TestCase):
    def setUp(self):
        # Switch threads as often as possible, so a race shows up here and not in production.
        self._interval = sys.getswitchinterval()
        sys.setswitchinterval(1e-6)

    def tearDown(self):
        sys.setswitchinterval(self._interval)

    def test_single_thread(self):
        m = Metrics()
        for _ in range(10):
            m.hit("/a")
        self.assertEqual(m.count("/a"), 10)
        self.assertEqual(m.snapshot(), {"/a": 10})

    def test_parallel_hits(self):
        m = Metrics()

        def worker():
            for _ in range(2000):
                m.hit("/a")

        threads = [threading.Thread(target=worker) for _ in range(8)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(m.count("/a"), 16000)


if __name__ == "__main__":
    unittest.main()
