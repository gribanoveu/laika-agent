import unittest

from app import config, report


class ReportTest(unittest.TestCase):
    def test_debug_report(self):
        text = report.build(config.load({"debug": True, "timeout": 5}))
        self.assertIn("timeout=5", text)
        self.assertIn("feature=audit", text)

    def test_plain_report(self):
        settings = config.load()
        self.assertFalse(settings["debug"])
        self.assertEqual(report.build(settings), "retries=3\ntimeout=30")


if __name__ == "__main__":
    unittest.main()
