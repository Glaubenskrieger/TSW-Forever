# Release pipeline: build on push, release on tag (GitHub Actions)

Status: implemented for version 0.4.0. Describes the owner-run release pipeline of the repository: what happens on a push to `main`, what the owner's release tag publishes, the key model, the GitHub settings and the recovery steps. Every step that changes the repository or its GitHub settings is done by the owner personally; the production signing key never leaves the owner's machine except as the passphrase-protected signing subkey in the protected environment `flatpak-release`.

## 1. Build on push, release on tag

**Phase A - the owner pushes to `main`: `build.yml` builds and checks everything and publishes nothing.** It starts for every push to `main` (and by hand on `main` or on an existing tag). Jobs: `flatpak` (`flatpak-stable-build.yml`: the full test suite first, then the unsigned stable OSTree repository and the deterministic source archive), `windows` (`windows.yml`: unsigned installer, tested silently), `candidate` (collects the artifacts of THIS run, writes `candidate.json` with version, full commit id and the SHA-256 of every file, `scripts/release_candidate.py`; the artifacts are kept **30 days**, the step summary shows the candidate) and `site` (the web site goes live on GitHub Pages). No key, no secret, no release, no tag, no Flatpak update: the signing and the stable channel are untouched by a push. `ci.yml` (formatting, Clippy, tests, e2e, policy) runs in parallel as before.

**The web site and the Flatpak data coexist on one Pages site.** Pages replaces the whole site with every deployment, so `site` first asks the live site for `channels.json`: 404 = no Flatpak release exists yet, the site is the web site alone (`assemble_pages_site.py --website-only`); 200 = the published OSTree repository, install files and `channels.json` are fetched from the live site file by file and verified against the `site-manifest.json` that the release deployment published (`scripts/preserve_live_site.py`), the repository is verified again with the committed public key, and deployed unchanged next to the new web site. Anything unexpected (HTTP status, missing manifest, a differing checksum) stops the job and the old site stays. `site` deploys only if its commit is still the tip of `main` (an older run never replaces a newer site) and runs in the concurrency group `pages-site`, shared with the release `deploy` job, so the two never interleave. The web site links no unpublished artifact: the download buttons read the latest PUBLISHED GitHub release.

**Phase B - the owner looks at the result:** the run `build` of the commit shows `flatpak`, `windows` and `candidate` green and the candidate in its summary; `ci` is green. Only then is the tag set.

**Phase C - the owner pushes the tag `v<APP_VERSION>` on exactly that commit: `release.yml` publishes the candidate.** It starts only for a tag `v<MAJOR>.<MINOR>.<PATCH>` pushed by the owner. There is no other trigger (no manual start, no `workflow_run`). **Nothing is compiled or tested again:** the job `candidate` finds the successful `build` run of exactly the tagged commit (never another commit, never "latest"), requires its artifacts to be complete and not expired, verifies every file against `candidate.json` and hands them to the next jobs. No run, a failed run, an expired artifact or a differing byte stops the release before anything is signed or published.

| Job | Runs on / needs | Rights | What it does |
| --- | --- | --- | --- |
| `guard` | - | read | The tag is the owner's, on `main`, equals `APP_VERSION`; actions are pinned; `packaging/flatpak/stable-repo.gpg` exists and is the pinned key (`flatpak_keys.sh check-pin`); the live channel state allows this version (no downgrade, no re-publish). |
| `candidate` | guard | read (`contents`, `actions`), no secret | Finds the successful `build.yml` run of exactly the tagged commit, checks that its artifacts exist and are not expired, downloads them, verifies them against `candidate.json` (`release_candidate.py verify`) and hands the verified unsigned repository, source archive and Windows installer on (7 days). |
| **`sign`** | guard, candidate | read (`contents`, `actions`); **environment `flatpak-release`: approval 1** | `scripts/flatpak_sign_ci.sh`: imports the signing SUBKEY (secret `TSW_FLATPAK_SIGNING_KEY`, passphrase `TSW_FLATPAK_SIGNING_PASSPHRASE`) into a throw-away keyring, checks the primary fingerprint and the subkey id, signs commits and summary, generates the static deltas afterwards, verifies with the committed public key and proves the update over the signed deltas. Uploads the signed repository (public data only). |
| `verify-signed` | candidate, sign | read, no key | Re-verifies with the public key only, builds the signed stable bundle (update source + public key inside), `check_bundle_matches_repo.sh`, installs the bundle and audits it. |
| `draft` | candidate, verify-signed | `contents: write` | Assembles the four release files (signed bundle, source archive, Windows installer, `SHA256SUMS`), `check_release_set.py --with-windows`, creates or continues the DRAFT, uploads, reads back and compares (`scripts/release_draft.sh upload`). |
| `pages-build` | sign, draft | read | Web site + signed repository + install files in one folder (`assemble_pages_site.py`), channel record. |
| `deploy` | pages-build | `pages`, `id-token`; environment `github-pages`; concurrency group `pages-site` | GitHub Pages: the web site and the Flatpak update source go live together. |
| `verify-live` | deploy, sign | read | `check_flatpak_live.sh`: every file is reachable and the LIVE repository verifies like a user's Flatpak (only the public key trusted, summary and commit signatures required, the expected commit), retried while Pages becomes reachable. |
| **`publish`** | draft, verify-live | `contents: write`; **environment `release-publish`: approval 2** | `scripts/release_draft.sh publish`: the draft is still a draft, holds exactly the four files and is byte-identical to the verified ones; it is published and read back (public, not a pre-release). |

Approval 1 comes after the unprivileged builds and immediately before the key is released. Approval 2 comes after the successful Pages deployment and the live check, immediately before the release page is published. **Consequence the owner accepted:** the Flatpak update source is public from the `deploy` job on, before the GitHub release exists; approval 2 gates the release page and the Windows installer, not the Flatpak update.

The release has exactly four files (no repository archive: the repository travels between jobs as a workflow artifact and goes live through Pages; later releases mirror the history from the live site).

## 2. Key model (the production keys exist; nothing in this repository creates a key)

The owner created and backed up the production keys. They are the only keys this pipeline accepts:

| What | Value |
| --- | --- |
| Primary key | fingerprint **`FAF3884F3E41D5C63FE656DC86075D3D53A6484B`** (key id `86075D3D53A6484B`), RSA 4096, certify only, no expiry. **Stays with the owner; never in GitHub.** |
| Signing subkey | key id **`43F56AAA62FBF7DA`**, RSA 4096, sign only, no expiry, protected by a passphrase. Only this subkey goes to GitHub. |
| Public key file | `packaging/flatpak/stable-repo.gpg`: public parts only (primary + this subkey), provided by the owner. |
| Pinned in the repository | `packaging/project.conf`: `FLATPAK_SIGNING_FINGERPRINT` and `FLATPAK_SIGNING_SUBKEY_ID` hold the two values above. |
| GitHub environment | **`flatpak-release`** (required reviewer: the owner = approval 1) |
| Environment secrets | **`TSW_FLATPAK_SIGNING_KEY`** (the signing subkey, base64 on one line, primary secret key only a stub), **`TSW_FLATPAK_SIGNING_PASSPHRASE`** |
| Environment variables | **`TSW_FLATPAK_SIGNING_FINGERPRINT`** (the full primary fingerprint), **`TSW_FLATPAK_SIGNING_SUBKEY_ID`** (the subkey id) |
| Approval 2 | environment **`release-publish`** (required reviewer: the owner), no secret, only the job `publish` |

Checks (all fail closed):

- `guard` (before anything is built, no secret): `scripts/flatpak_keys.sh check-pin packaging/flatpak/stable-repo.gpg packaging/project.conf` - the file holds public parts only, exactly one key, whose primary fingerprint and signing subkey id are the pinned ones.
- `sign` (`scripts/flatpak_sign_ci.sh`): the variables must be well-formed (40 and 16 hex digits) and equal the values pinned in `packaging/project.conf`; the imported secret must be a signing subkey ONLY (primary secret key a stub `sec#`, exactly one secret subkey, capability sign only, key id = `TSW_FLATPAK_SIGNING_SUBKEY_ID`, primary fingerprint = `TSW_FLATPAK_SIGNING_FINGERPRINT`); the committed public key must contain exactly this primary key and this subkey; the passphrase must unlock the subkey (a test signature) before the repository is touched; tracing (`set -x`) is refused. Then it signs with exactly that subkey (`<id>!`), generates the static deltas afterwards, verifies with the committed public key, proves the update over the signed deltas and scans for key material.
- The keyring is a throw-away mode-700 folder in `RUNNER_TEMP`, outside the repository, with its own gpg-agent; both are removed on every exit path. The secret key is read from the environment and passed through a pipe from a shell builtin; the passphrase is handed to that agent through a pipe (`gpg-preset-passphrase`) so that Flatpak's own gpg call needs no prompt, and is unset right after. Neither is ever in an argument, a file, the output or an artifact (tests check output, repository and temporary folder).
- The job `sign` uses only distribution packages (`flatpak ostree gnupg gpg-agent python3`) and the GitHub actions `checkout` and `upload-artifact` (pinned). It compiles nothing and downloads nothing from third parties. The policy check (`scripts/check_flatpak_package.py ci-policy`, `check_tag_release_workflow`) and its mutation tests enforce: the two secrets and two variables appear in exactly one step of that job, behind `flatpak-release`, with nothing around them.
- No expiry: a Flatpak client stores the public key once and never refreshes it, so an extended expiry would not reach installed clients. Protection comes from revocation with the offline primary key and from rotation (`documentation/packaging.md`).
- Throw-away keys exist only in automated tests (`test_flatpak_keys.py`, `test_flatpak_sign_ci.py`, the end-to-end test, `release-dry-run.yml`) and never reach a production secret.
- The GitHub secrets cannot be read back: the owner's own backups are the only copy.

## 3. Providing the keys and the GitHub settings (owner only; no private key, backup or passphrase is ever shared)

### 3.1 The subkey export (on the machine that holds the keys; run these yourself)

```bash
# Only the signing SUBKEY (the "!" selects exactly this subkey), no primary secret key; gpg asks for the passphrase.
gpg --armor --export-secret-subkeys '43F56AAA62FBF7DA!' | base64 -w0 > "$HOME/tsw-ci-subkey.b64"
# Check the file BEFORE it leaves your machine (prints ids and a verdict, never a secret):
scripts/flatpak_keys.sh inspect-ci-export "$HOME/tsw-ci-subkey.b64"
#   expected: secret subkey id: 43F56AAA62FBF7DA / primary fingerprint: FAF3884F3E41D5C63FE656DC86075D3D53A6484B / primary secret key: stub only (good)
# The public key file (public parts only; the tool refuses to export a secret):
scripts/flatpak_keys.sh export-public "$GNUPGHOME" FAF3884F3E41D5C63FE656DC86075D3D53A6484B packaging/flatpak/stable-repo.gpg
scripts/flatpak_keys.sh check-pin packaging/flatpak/stable-repo.gpg packaging/project.conf
```

`inspect-ci-export` exits 1 if the file holds the primary secret key, a full secret key, more than one secret subkey or a subkey that can do more than sign: then do not upload it. Put `packaging/flatpak/stable-repo.gpg` into the commit. The file `tsw-ci-subkey.b64` is a secret: set it as the environment secret below and delete it afterwards (`shred -u`). If the subkey itself is not protected by a passphrase, or the passphrase is empty, the secret `TSW_FLATPAK_SIGNING_PASSPHRASE` may be empty; the production subkey has a passphrase.

### 3.2 Settings in the public repository (browser or your own `gh` session; owner only)

1. Repository public; **Pages: Source "GitHub Actions"**.
2. **Environments:**
   - `flatpak-release`: required reviewer = you; "Prevent self-review" OFF (a single maintainer could not approve otherwise); deployment branches and tags: tag pattern `v*.*.*` only; secrets `TSW_FLATPAK_SIGNING_KEY` and `TSW_FLATPAK_SIGNING_PASSPHRASE`; variables `TSW_FLATPAK_SIGNING_FINGERPRINT` = `FAF3884F3E41D5C63FE656DC86075D3D53A6484B` and `TSW_FLATPAK_SIGNING_SUBKEY_ID` = `43F56AAA62FBF7DA`.
   - `release-publish`: required reviewer = you; tag pattern `v*.*.*`; no secret.
   - `github-pages`: deployment from branch `main` (web site on every push) AND tags `v*` (the release deployment) - both are set.
   With the `gh` of your own session (nothing secret on a command line): `gh secret set TSW_FLATPAK_SIGNING_KEY --env flatpak-release --repo Glaubenskrieger/TSW-Forever < "$HOME/tsw-ci-subkey.b64"`; `gh secret set TSW_FLATPAK_SIGNING_PASSPHRASE --env flatpak-release --repo Glaubenskrieger/TSW-Forever` (asks for the value, hidden); `gh variable set TSW_FLATPAK_SIGNING_FINGERPRINT --env flatpak-release --repo Glaubenskrieger/TSW-Forever --body FAF3884F3E41D5C63FE656DC86075D3D53A6484B`; `gh variable set TSW_FLATPAK_SIGNING_SUBKEY_ID --env flatpak-release --repo Glaubenskrieger/TSW-Forever --body 43F56AAA62FBF7DA`.
3. **Rulesets:** tag `v*` (only you may create; no update, no delete); `main` (no force push).
4. **Actions:** allow only GitHub-owned actions and pinned SHAs; default token permission read.
5. Secret scanning with push protection on.

### 3.3 Files of the repository and the owner's commands

`.github/workflows/{build,release,flatpak-stable-build,release-dry-run,flatpak,windows}.yml` (the old `release-flatpak.yml` is removed), `packaging/project.conf` (pin), `packaging/flatpak/stable-repo.gpg` (the owner's public key, already there), the scripts `flatpak_keys.sh`, `flatpak_sign_ci.sh`, `check_flatpak_live.sh`, `check_bundle_matches_repo.sh`, `release_draft.sh`, `release_candidate.py`, `preserve_live_site.py`, `flatpak_channel.py`, `gen_flatpak_refs.sh`, `assemble_pages_site.py`, `flatpak_mirror_live.sh`, `check_flatpak_delta_update.sh`, their tests, `website/`.

**The owner's commands (fish), in this order:** `git add -A; git commit; git push origin main` (Phase A), wait until `build` and `ci` of the commit are green (`gh run list --commit (git rev-parse HEAD)`), then `git tag v0.4.0; git push origin v0.4.0` (Phase C), approve `flatpak-release` when asked (signing), approve `release-publish` when asked (publication). **Do not push anything to `main` between the tag push and the end of the `publish` job, plus about ten minutes:** a web site deployment that reads a cached copy of the live site (Pages sits behind a cache) could deploy the old state over the new repository. The fetches of `channels.json` and `site-manifest.json` carry a cache-busting query string; whether Pages honours it is not verified (open point).

## 4. Testing

1. **Local, before every push** (`scripts/build_flatpak.sh test`, `scripts/e2e_flatpak_update.sh`): all new scripts are tested with throw-away keys (`test_flatpak_keys.py` incl. the split-key model, `test_flatpak_sign_ci.py`, `test_check_flatpak_live.py` against a local web server with every defect, `test_check_bundle_matches_repo.py`, `test_release_draft.py` with a stub of `gh`, `test_check_flatpak_package.py` with mutation tests for every rule of the job graph). The end-to-end test signs version 2 through `flatpak_sign_ci.sh` with the subkey only.
2. **`release-dry-run.yml`** (manual, `main`, no secret, no environment, no write): the same scripts on a real runner with a throw-away key made inside the job: unsigned build, signing, verification, bundle, installation audit, site assembly and the live check against a local web server. Nothing is deployed, tagged or published. In the private development repository it needs only the Actions minutes (the environments and Pages are not needed).
3. **Not testable before the first real run:** that GitHub releases the environment secret only after approval 1 (documented GitHub behaviour, enforced by GitHub), that `github-pages` accepts a deployment from a tag (setup step 3.2), and that `gh` handles a draft by its tag with the job token (the private release rehearsal of 0.3.0 used the same calls). Every failure before the `deploy` job leaves nothing public.
4. A complete rehearsal with real environments, Pages and a draft is possible in a scratch public repository of the owner with a throw-away key (owner action).

## 5. Recovery

| Situation | State | What to do |
| --- | --- | --- |
| `candidate` fails: no successful `build` run for the tagged commit | nothing public | The commit was never pushed to `main`, or its build failed: push it / fix it, wait for a green `build`, then re-run the release workflow. A tag on a commit without a green build is never published. Do not move the tag. |
| `candidate` fails: artifacts expired (30 days) | nothing public | `gh workflow run build.yml --ref v0.4.0` (rebuilds the same commit from the tag; `site` is skipped on a tag), wait for green, re-run the release workflow. |
| A job fails before `draft` (signing, verification) | nothing public | Fix, then "Re-run failed jobs"; approval 1 is asked again for `sign`. |
| `draft` fails halfway | an incomplete DRAFT, nothing public | Re-run the job: it continues the draft (same names replaced), refuses foreign files and a published release. |
| `deploy` fails | the previous site stays (first release: no site); Pages deploys atomically | Re-run only `deploy` (the site artifact is kept 7 days); check the `github-pages` environment rules. |
| `verify-live` fails | the site may be live, the release is a draft | Re-run (Pages needs minutes); if the content is wrong: fix forward with a new patch version (the Flatpak keeps the previous commit; users can go back with `--commit`, `documentation/user-guide.md`). |
| `publish` fails or is rejected | the site is live, the release is a draft | Re-run only `publish`. A re-run of the whole workflow is refused by "strictly newer than live" (`flatpak_channel.py plan`), intended. |
| The tag was wrong | nothing signed without approval 1 | Reject approval 1; do not move or delete a published tag; a corrected release needs a new version. |
| Abandoned release | a draft remains | Delete the draft by hand (the tag stays). |
| Key compromise | - | Revoke with the offline primary key, delete the secret, create a new subkey, rotate as in `documentation/packaging.md`. |
| Key loss (no backup) | no further update can be signed | Existing installations stop updating until users install a bundle with a new key. Keep two backups. |

## 6. Limits and open points

- A Pages deployment carries the whole site: the carried-over Flatpak data comes from the live site and is verified against `site-manifest.json` (written from the first release on) and the public key. A site that was published without a manifest cannot be carried over: the `site` job stops and the old site stays.
- Artifacts of a push are kept 30 days. GitHub Free: the Actions storage quota is shared by all artifacts; the three release artifacts are re-uploaded for 7 days by the tag run.
- The release has no pre-release stage; the Flatpak update source is public from `deploy` on (accepted).
- `packaging/flatpak/stable-repo.gpg` must be part of the commit (`build` stops without it).
- `scripts/flatpak_release_offline.sh` stays as a test and emergency tool and is marked "not for publication"; the regular process never uses it.
- GitHub-side behaviour (environments, Pages from a tag) is documented but untested here; see section 4.
