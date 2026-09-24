"""Everything the class promises, under threads — not only the one test that failed."""
import sys
import threading
import time
import unittest

from metrics import Metrics


def run(workers):
    threads = [threading.Thread(target=w) for w in workers]
    for t in threads:
        t.start()
    for t in threads:
        t.join()


class Hidden(unittest.TestCase):
    def setUp(self):
        self._interval = sys.getswitchinterval()
        sys.setswitchinterval(1e-6)

    def tearDown(self):
        sys.setswitchinterval(self._interval)

    def test_many_routes_exact(self):
        for _ in range(5):
            m = Metrics()
            run([lambda r=f"/{i % 3}": [m.hit(r) for _ in range(1500)] for i in range(9)])
            self.assertEqual(m.snapshot(), {"/0": 4500, "/1": 4500, "/2": 4500})

    def test_take_loses_nothing(self):
        """What the exporter took plus what is left is every hit — the docstring's promise."""
        for _ in range(3):
            m = Metrics()
            taken = []
            done = threading.Event()

            def exporter():
                while not done.is_set():
                    taken.append(m.take("/a"))

            reader = threading.Thread(target=exporter)
            reader.start()
            run([lambda: [m.hit("/a") for _ in range(3000)] for _ in range(6)])
            done.set()
            reader.join()
            self.assertEqual(sum(taken) + m.count("/a"), 18000)

    def test_last_seen_still_recorded(self):
        m = Metrics()
        m.hit("/a")
        self.assertIsNotNone(m.last_seen("/a"))
        m.reset("/a")
        self.assertEqual(m.count("/a"), 0)
        self.assertIsNone(m.last_seen("/a"))

    def test_not_made_slow(self):
        m = Metrics()
        started = time.monotonic()
        run([lambda: [m.hit("/a") for _ in range(5000)] for _ in range(8)])
        self.assertEqual(m.count("/a"), 40000)
        self.assertLess(time.monotonic() - started, 5)


if __name__ == "__main__":
    unittest.main()
