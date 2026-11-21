# Release pipeline: build on push, release on tag (GitHub Actions)

Status: implemented for version 0.4.1. Describes the owner-run release pipeline of the repository: what happens on a push to `main`, what the owner's release tag publishes, the two human approvals, the GitHub settings and the recovery steps. Every step that changes the repository or its GitHub settings (commits, pushes, tags, approvals, environments) is done by the owner personally; no agent and no workflow does it.

Since 0.4.1 the distribution is simple: **GitHub Pages carries the static web site only; the installation files are assets of the GitHub release.** There is no hosted Flatpak/OSTree repository, no GPG signing and no secret in any workflow.

## 1. Build on push, release on tag

**Phase A - the owner pushes to `main`: `build.yml` builds and checks everything and publishes nothing.** It starts for every push to `main` (and by hand on `main` or on an existing tag). Jobs:

| Job | What it does |
| --- | --- |
| `flatpak` | `flatpak-stable-build.yml`: the full test suite first, then the release Flatpak bundle (`tsw-forever-<version>.flatpak`, branch `local` like every earlier bundle, built with the release build metadata `TSW_RELEASE_BUILD=1`, no update source, no key), installed once and audited (starts, reports the version, branch `local`, both programs carry the release build marker, no remote with an address, no Microsoft/Funcom file, no game data), and the deterministic source archive. Artifacts for 30 days. |
| `windows` | `windows.yml`: the Windows installer (`tsw-forever-<version>-windows-setup.exe`) and its checksum file. |
| `candidate` | collects the artifacts of THIS run, writes `candidate.json` (version, full commit id, SHA-256 of every file; `scripts/release_candidate.py`), audits the source archive and checks that `CHANGELOG.md` has an entry for the version. Kept 30 days. |
| `site` | independent of the three jobs above: tests and deploys the static web site (`website/`) to GitHub Pages. Only for a push to `main`, only the tip of `main`, in the concurrency group `pages-site`. The artifact is the web site only (`scripts/assemble_pages_site.py` refuses an installation or repository file) and a few hundred kilobytes. |

`build.yml` creates no release, no tag and no commit, holds no secret, and uses no approval environment. The web site never waits for a release and a release never waits for the web site: the download buttons ask GitHub for the newest PUBLISHED release when the page opens (`releases/latest` never returns a draft or a pre-release; the page accepts only exact file names of the same version and repository addresses; before the first release the buttons are switched off with an explanatory text; if GitHub cannot be asked, they open the list of releases).

**Phase B - the owner looks at the result:** the run `build` of the commit shows `flatpak`, `windows` and `candidate` green and the candidate in its summary; `ci` is green. Only then is the tag set.

**Phase C - the owner pushes the tag `v<APP_VERSION>` on exactly that commit: `release.yml` publishes the candidate.** It starts only for a tag `v<MAJOR>.<MINOR>.<PATCH>` pushed by the owner (`github.actor`/`triggering_actor` = the repository owner, `push`, `tag`). There is no other trigger (no manual start, no `workflow_run`). **Nothing is compiled or tested again:** the tag publishes exactly the files that `build.yml` built for that commit.

| Job | Needs / rights | What it does |
| --- | --- | --- |
| `guard` | read | tag is `vX.Y.Z`, the commit is on `main`, the tag equals `APP_VERSION`, actions are pinned, and the version is NEWER than the newest published release (no downgrade, no re-publish; drafts do not count). |
| `candidate` | guard; read (`contents`, `actions`) | finds the successful `build` run of EXACTLY the tagged commit, requires every artifact to exist and not be expired, downloads them from that run and verifies every file against `candidate.json` (version, commit, SHA-256; no missing, extra or linked file). A newer commit's or another run's artifacts are never used. |
| **`approve`** | guard, candidate; read; **environment `flatpak-release`: approval 1** | the approval of the WHOLE release, Linux and Windows together. GitHub holds the job until a required reviewer approves (and sends the notification). The job runs no action, no project script and uses no secret; it only prints that the release was approved. |
| `draft` | candidate, approve; `contents: write` | assembles the four release files (`tsw-forever-<V>.flatpak`, `tsw-forever-<V>-src.tar.gz`, `tsw-forever-<V>-windows-setup.exe`, `SHA256SUMS`), checks the checksums and the set (`check_release_set.py --with-windows`), audits the source archive, extracts the release notes from `CHANGELOG.md` and fills a DRAFT release; the draft is read back and compared byte for byte (`scripts/release_draft.sh upload`). |
| **`publish`** | draft; `contents: write`; **environment `release-publish`: approval 2** | verifies the draft once more (still a draft, exactly four files, byte-identical), publishes it and reads the result back (`scripts/release_draft.sh publish`). |

Nothing is public before approval 2 (a draft is not visible to users and `releases/latest` ignores it). A failed check stops the chain before anything is published: a failed test, a missing or expired build, a changed file, a refused approval.

## 2. The two approvals (the gates)

Both are GitHub environments with **required reviewers**, enforced by GitHub, not by the workflow file. They are the owner's settings; this repository, its workflows and its agents never change them (`scripts/check_flatpak_package.py ci-policy` only checks that the workflow still uses them in the right places and that `build.yml` does not).

* **`flatpak-release`** (approval 1, the historical name is kept): job `approve`, before anything is put into a draft. It now covers the whole release, Linux and Windows together. Settings that are still useful: required reviewer = the owner, deployment branches/tags limited to `v*`. Any secret or variable that this environment still holds from the 0.4.0 plan (`TSW_FLATPAK_SIGNING_*`) is UNUSED: no workflow references a secret any more (policy-checked); the owner may delete them, and the keys, at any time.
* **`release-publish`** (approval 2): job `publish`, before the draft is published. Required reviewer = the owner, tags `v*`.

The e-mail/notification of GitHub for a waiting deployment is unchanged. A refused or timed-out approval stops the release; the draft (if it exists) stays a draft.

## 3. GitHub settings (owner only)

Settings that the pipeline needs (set or confirm them yourself; nothing here is done by a workflow or an agent):

* Pages: source "GitHub Actions"; environment `github-pages` allows `main` (the web site deployment of `build.yml`, job `site`). A tag deployment is not used any more.
* Environments `flatpak-release` and `release-publish`: required reviewer, tags `v*` (see section 2).
* Actions: default workflow permissions read-only; actions pinned by commit SHA (`scripts/pin_actions.sh --check`).
* No secret and no variable is needed.

## 4. The owner's commands for 0.4.1 (fish)

Check the change set first: `git status`, `git diff --stat`, and set the date of the 0.4.1 entry in `CHANGELOG.md` and the release date in `packaging/flatpak/org.tswlauncher.Launcher.metainfo.xml` if they should differ.

```fish
cd ~/Projekte/tsw-forever
git status --short
git add -A
git commit -m "Release 0.4.1: website-based downloads, no hosted Flatpak repository"
git push origin main
gh run list --branch main --limit 5
gh run watch (gh run list --workflow build.yml --branch main --limit 1 --json databaseId --jq '.[0].databaseId')
gh run list --workflow ci.yml --commit (git rev-parse HEAD)
# only when build and ci of exactly this commit are green:
git tag v0.4.1
git push origin v0.4.1
# GitHub now waits twice for you: approve the environment flatpak-release, then release-publish
```

After `publish`: open https://github.com/Glaubenskrieger/TSW-Forever/releases and https://glaubenskrieger.github.io/TSW-Forever/ and check that the buttons of the web site download the files of the new release. The unpublished draft of 0.4.0 stays untouched until you decide what to do with it (delete it, or leave it; it does not matter for the web site or the launcher because drafts are never offered).

## 5. Testing

Local (all run by `scripts/build_flatpak.sh test` and `ci.yml`): `test_release_candidate.py` (manifest of the candidate), `test_release_draft.py` (draft, publish and their refusals with a stub `gh`), `test_assemble_pages_site.py` and `test_website.py` plus `test_website_download.js` (the web site, every kind of GitHub answer for the download buttons), `test_check_flatpak_package.py` (the workflow policy with mutation tests: both approvals, no secret, no key, no Pages deployment in `release.yml`, only the web site in the Pages artifact), `launcher-gui/tests/update_paths.rs` (the launcher cannot install an update), and `scripts/e2e_flatpak_bundle.sh` (the bundle install and update path with the real Flatpak).

**Not testable before the first real run:** that the environments really stop for the approval (GitHub's behaviour), the cross-run artifact download and `gh run list --commit` of the job `candidate`, the Pages deployment of `build.yml`, the draft handling of `release_draft.sh` with the real `gh`, and the web site's download buttons against a real release. The first push and the first tag show them; a failure before `publish` leaves nothing public.

## 6. Recovery

| Situation | What to do |
| --- | --- |
| `candidate` says there is no successful build run for the commit | Wait for `build`, or start it: `gh workflow run build.yml --ref v0.4.1`. Then re-run the failed jobs of the release run. Nothing was published. |
| An artifact of the build run expired (30 days) | `gh workflow run build.yml --ref <tag>` (pushes nothing), then re-run the release run. |
| A check fails (a test, the set, the changelog entry) | Fix it in a new commit on `main`; the old tag has no run to use. Delete the tag yourself if you want to reuse the number (only while nothing was published). |
| You refuse approval 1 or 2 | The release stops; the draft (if any) stays a draft. Re-run the job to ask again, or delete the draft. |
| `publish` failed after the release became public | Check the release page. A published release is never touched by the pipeline; correct the notes by hand if needed. |
| The web site shows "no release yet" or opens the release list | The buttons ask GitHub when the page opens; a published release appears at once for new visitors. A rate limit makes the buttons open the list of releases instead; reload later. |

## 7. Limits and open points

* The integrity of the downloaded files rests on `SHA256SUMS` and GitHub's release hosting; the Flatpak bundle carries no signature and the Windows installer is not code-signed (`security.md`).
* Flatpak installs an older bundle without a warning (`scripts/e2e_flatpak_bundle.sh`, B4); the web site always links to the newest release.
* The web site's buttons depend on the GitHub API (60 anonymous requests per hour and address): beyond that they open the list of releases.
* `ci.yml` also builds a development Flatpak on `main` (a duplicate compile that never conflicts with the candidate).
* The Windows 0.3.0 to 0.4.x upgrade test and the real Flatpak build of 0.4.1 on a clean system are to be done by the owner before the tag.
