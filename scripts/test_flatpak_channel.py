#!/usr/bin/env python3
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import flatpak_channel as c  # noqa: E402

H = "a" * 64


def live(**kw):
    return {k: {"version": v, "commit": H} for k, v in kw.items()}


class Plan(unittest.TestCase):
    def test_prerelease_goes_to_testing_only_and_never_to_stable(self):
        self.assertEqual(c.plan("0.3.0-beta.1", "testing", {}), ["testing"])
        with self.assertRaises(ValueError):
            c.plan("0.3.0-beta.1", "stable", {})

    def test_first_stable_release_also_fills_testing(self):
        self.assertEqual(c.plan("1.0.0", "stable", {}), ["stable", "testing"])

    def test_a_final_stable_release_is_promoted_to_testing_only_if_newer(self):
        self.assertEqual(c.plan("1.1.0", "stable", live(stable="1.0.0", testing="1.1.0-rc.1")), ["stable", "testing"])
        self.assertEqual(c.plan("1.1.0", "stable", live(stable="1.0.0", testing="1.2.0-beta.1")), ["stable"])

    def test_testing_may_receive_a_final_version_first(self):
        self.assertEqual(c.plan("1.1.0", "testing", live(stable="1.0.0", testing="1.1.0-rc.2")), ["testing"])

    def test_no_downgrade_and_no_republish(self):
        for v in ("1.0.0", "0.9.0"):
            with self.assertRaises(ValueError):
                c.plan(v, "stable", live(stable="1.0.0"))
        with self.assertRaises(ValueError):
            c.plan("1.1.0-beta.1", "testing", live(testing="1.1.0-beta.1"))
        with self.assertRaises(ValueError):
            c.plan("1.1.0-beta.1", "testing", live(testing="1.1.0"))  # 1.1.0 > 1.1.0-beta.1

    def test_garbage_is_refused(self):
        for v, ch in (("1.0", "stable"), ("1.0.0", "nightly"), ("", "testing")):
            with self.assertRaises(ValueError):
                c.plan(v, ch, {})


class File(unittest.TestCase):
    def test_record_and_load_round_trip_and_reject_bad_input(self):
        p = os.path.join(tempfile.mkdtemp(prefix="tsw-chan-"), "channels.json")
        self.assertEqual(c.load(p), {})
        c.record(p, "testing", "0.3.0-beta.1", H)
        c.record(p, "stable", "0.2.0", "b" * 64)
        self.assertEqual(c.load(p)["testing"]["version"], "0.3.0-beta.1")
        self.assertEqual(list(c.load(p)), ["stable", "testing"])
        for bad in (("nightly", "1.0.0", H), ("stable", "1.0", H), ("stable", "1.0.0", "xyz")):
            with self.assertRaises(ValueError):
                c.record(p, *bad)

    def test_unknown_schema_or_channel_in_the_file_is_an_error(self):
        p = os.path.join(tempfile.mkdtemp(prefix="tsw-chan-"), "channels.json")
        open(p, "w").write('{"schema": 2, "channels": {}}')
        with self.assertRaises(ValueError):
            c.load(p)
        open(p, "w").write('{"schema": 1, "channels": {"nightly": {"version": "1.0.0", "commit": "x"}}}')
        with self.assertRaises(ValueError):
            c.load(p)


if __name__ == "__main__":
    unittest.main()
