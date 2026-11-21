"""Shared helper of the tests that need programs from the machine (flatpak, gpg, ostree, ...).

On a developer machine without them such a test class is SKIPPED, so the rest of the chain still runs. In a CI job that is meant to run the security tests (the signing, the mirror, the
delta update and the release tests) the environment variable TSW_REQUIRE_TOOLS=1 is set: a missing program is then a FAILURE with a clear message, never a silent skip. (A silent skip once
meant that 38 tests of the signing and update chain passed without running in the job that has no Flatpak tools.)

Only a missing PROGRAM is handled here. A test that cannot apply for another reason (no release set in the tree, no built repository, no copy of the game client) keeps its own, explained skip.

Use:  sys.path.insert(0, HERE); from testtools import requires_tools   and   @requires_tools("flatpak", "gpg", "ostree")   on the TestCase class.
"""
import os
import shutil
import unittest

REQUIRE_ENV = "TSW_REQUIRE_TOOLS"


def missing(tools):
    """The programs of `tools` that are not on the PATH."""
    return [t for t in tools if shutil.which(t) is None]


def required():
    """True where the environment says that the tests of this run must not be skipped for a missing program."""
    return os.environ.get(REQUIRE_ENV) == "1"


def requires_tools(*tools):
    """Class decorator: skip the class where a program is missing, or FAIL when TSW_REQUIRE_TOOLS=1 (the CI jobs that run the security tests)."""
    gone = missing(tools)
    if not gone:
        return lambda cls: cls
    if not required():
        return unittest.skipIf(True, f"missing tools: {gone}")

    def fail(cls):
        message = (
            f"the tools {gone} are missing, but {REQUIRE_ENV}=1 says that these tests must run: install them in this job "
            f"(for the Flatpak tests: flatpak ostree gnupg) instead of letting the signing, mirror and delta tests be skipped silently"
        )

        def test_the_required_tools_are_installed(self):
            self.fail(message)

        return type(cls.__name__, (unittest.TestCase,), {"__doc__": cls.__doc__, "test_the_required_tools_are_installed": test_the_required_tools_are_installed})

    return fail
