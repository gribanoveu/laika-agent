import unittest

from app import config


class Hidden(unittest.TestCase):
    def test_a_loaded_copy_is_its_own(self):
        first = config.load({"debug": True})
        first["features"].append("x")
        first["retries"] = 9
        again = config.load()
        self.assertEqual(again["features"], [])
        self.assertEqual(again["retries"], 3)
        self.assertFalse(again["debug"])

    def test_overrides_still_apply(self):
        self.assertEqual(config.load({"timeout": 1})["timeout"], 1)
        self.assertEqual(config.load({"features": ["a"]})["features"], ["a"])


if __name__ == "__main__":
    unittest.main()
