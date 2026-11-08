#!/usr/bin/env python3
"""Test of ci_disk_report.sh: it reports free space, inodes and directory sizes, skips missing paths, appends one row per call to the job summary and changes nothing else."""
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
SCRIPT = os.path.join(HERE, "ci_disk_report.sh")


def run(args, summary=None, cwd=None):
    env = {"PATH": os.environ["PATH"], "HOME": cwd or "/nonexistent", "RUNNER_TEMP": cwd or tempfile.gettempdir()}
    if summary:
        env["GITHUB_STEP_SUMMARY"] = summary
    return subprocess.run(["bash", SCRIPT, *args], capture_output=True, text=True, env=env, cwd=cwd)


class DiskReport(unittest.TestCase):
    def test_it_reports_space_inodes_and_the_size_of_existing_paths_and_skips_missing_ones(self):
        with tempfile.TemporaryDirectory() as t:
            os.makedirs(os.path.join(t, "work"))
            open(os.path.join(t, "work", "data.bin"), "wb").write(b"x" * 200_000)
            p = run(["after cleanup 1", os.path.join(t, "work"), os.path.join(t, "missing")], cwd=t)
            self.assertEqual(p.returncode, 0, p.stderr)
            self.assertIn("=== disk report: after cleanup 1 ===", p.stdout)
            self.assertRegex(p.stdout, r"df -h .*Avail")
            self.assertRegex(p.stdout, r"df -i .*IFree")
            self.assertRegex(p.stdout, r"du -sh\s+\S+\s+" + os.path.join(t, "work"))
            self.assertNotIn("missing", p.stdout)

    def test_it_uses_the_build_directories_by_default_and_never_fails_on_an_empty_checkout(self):
        with tempfile.TemporaryDirectory() as t:
            p = run(["runner start"], cwd=t)
            self.assertEqual(p.returncode, 0, p.stderr)
            self.assertIn("df -i", p.stdout)

    def test_every_call_appends_one_row_to_the_job_summary_under_one_header(self):
        with tempfile.TemporaryDirectory() as t:
            summary = os.path.join(t, "summary.md")
            open(summary, "w").write("## other step output\n")
            run(["before cleanup", t], summary=summary, cwd=t)
            run(["after cleanup", t], summary=summary, cwd=t)
            text = open(summary).read()
            self.assertEqual(text.count("| measuring point | free | used | free inodes | change since previous |"), 1)
            rows = [l for l in text.splitlines() if l.startswith("| before cleanup |") or l.startswith("| after cleanup |")]
            self.assertEqual(len(rows), 2, text)
            for row in rows:
                cells = [c.strip() for c in row.strip("|").split("|")]
                self.assertEqual(len(cells), 5, row)
                self.assertRegex(cells[1], r"^\d")
                self.assertRegex(cells[2], r"^\d+%$")
                self.assertRegex(cells[3], r"^\d+$")
                self.assertRegex(cells[4], r"^(n/a|[+-]\d+ MiB)$")

    def test_it_changes_nothing_in_the_inspected_directories_and_only_adds_its_state_file(self):
        with tempfile.TemporaryDirectory() as t:
            d = os.path.join(t, "build")
            os.makedirs(d)
            open(os.path.join(d, "keep.txt"), "w").write("keep")
            before_top = set(os.listdir(t))
            run(["x", d], cwd=t)
            self.assertEqual(os.listdir(d), ["keep.txt"])
            self.assertEqual(open(os.path.join(d, "keep.txt")).read(), "keep")
            self.assertEqual(set(os.listdir(t)) - before_top, {"ci_disk_report.last"})

    def test_the_change_since_the_previous_measuring_point_is_reported(self):
        with tempfile.TemporaryDirectory() as t:
            first = run(["first"], cwd=t)
            self.assertIn("change since the previous measuring point: n/a", first.stdout)
            self.assertTrue(os.path.isfile(os.path.join(t, "ci_disk_report.last")))
            with open(os.path.join(t, "big.bin"), "wb") as f:
                f.write(b"\0" * 20_000_000)
            second = run(["second"], cwd=t)
            self.assertRegex(second.stdout, r"change since the previous measuring point: [+-]\d+ MiB")

    def test_a_missing_label_is_an_error(self):
        self.assertNotEqual(run([]).returncode, 0)


if __name__ == "__main__":
    unittest.main()
