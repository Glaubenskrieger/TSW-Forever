#!/usr/bin/env python3
"""Channel rules of the TSW-Forever Flatpak repository (stable / testing). Pure functions plus a small CLI; no network, no Git, no Flatpak needed.

The channels are Flatpak BRANCHES of the same application ID (ref `app/<id>/<arch>/<branch>`), so there is no second version scheme: the version
is `APP_VERSION` of packaging/project.conf (scripts/version.py), the branch only says who receives it.

  stable   every final version the owner releases; the default of the .flatpakref.
  testing  pre-releases and final versions the owner puts there first. A FINAL version published to stable is promoted to testing as well, so a
           testing user is never stranded on code older than stable (and never gets a downgrade from an older stable release).

Rules enforced here (and by the release workflow):
  * a pre-release (1.2.0-beta.1) can only go to testing;
  * a version is published only if it is strictly newer (SemVer precedence) than what the branch carries now (`channels.json` of the live site);
  * the promotion to testing happens only if the new version is newer than the current testing version, otherwise testing keeps its newer one.

  flatpak_channel.py plan <version> <stable|testing> [<channels.json>]   print the branches to publish (one per line); exit 1 = refused (reason on stderr)
  flatpak_channel.py record <channels.json> <branch> <version> <commit>   add/replace one branch entry (creates the file)
  flatpak_channel.py show <channels.json>
`channels.json` is a hint for tooling and for the publish guard; the trust anchor stays the GPG signature of the OSTree commits.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import version as V  # noqa: E402

CHANNELS = ("stable", "testing")
SCHEMA = 1


def load(path):
    """-> {branch: {"version": str, "commit": str}}; a missing or empty file means "nothing published yet"."""
    if not path or not os.path.exists(path):
        return {}
    with open(path) as f:
        data = json.load(f)
    if data.get("schema") != SCHEMA:
        raise ValueError(f"unsupported channels.json schema {data.get('schema')!r}")
    out = data.get("channels", {})
    for b, e in out.items():
        if b not in CHANNELS:
            raise ValueError(f"unknown channel {b!r}")
        V.parse(e["version"])
    return out


def plan(version, channel, live):
    """Branches to publish for `version` requested for `channel`, given the live state; raises ValueError with the reason if refused."""
    if channel not in CHANNELS:
        raise ValueError(f"unknown channel {channel!r} (stable or testing)")
    V.parse(version)
    pre = V.is_prerelease(version)
    if channel == "stable" and pre:
        raise ValueError(f"{version} is a pre-release; stable receives final versions only")
    cur = live.get(channel)
    if cur and V.compare(version, cur["version"]) <= 0:
        raise ValueError(f"{channel} already carries {cur['version']}; {version} is not newer (no downgrade, no re-publish)")
    branches = [channel]
    if channel == "stable":
        t = live.get("testing")
        if not t or V.compare(version, t["version"]) > 0:
            branches.append("testing")  # promotion: testing never falls behind stable
    return branches


def record(path, branch, version, commit):
    if branch not in CHANNELS:
        raise ValueError(f"unknown channel {branch!r}")
    V.parse(version)
    if not (len(commit) == 64 and all(c in "0123456789abcdef" for c in commit)):
        raise ValueError("commit must be a 64-character OSTree checksum")
    live = load(path)
    live[branch] = {"version": version, "commit": commit}
    with open(path, "w") as f:
        json.dump({"schema": SCHEMA, "channels": {b: live[b] for b in CHANNELS if b in live}}, f, indent=2, sort_keys=True)
        f.write("\n")


def main(argv):
    try:
        if len(argv) in (4, 5) and argv[1] == "plan":
            for b in plan(argv[2], argv[3], load(argv[4] if len(argv) == 5 else None)):
                print(b)
            return 0
        if len(argv) == 6 and argv[1] == "record":
            record(argv[2], argv[3], argv[4], argv[5])
            return 0
        if len(argv) == 3 and argv[1] == "show":
            print(json.dumps(load(argv[2]), indent=2, sort_keys=True))
            return 0
    except (ValueError, KeyError, json.JSONDecodeError) as e:
        print(f"flatpak_channel: {e}", file=sys.stderr)
        return 1
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
