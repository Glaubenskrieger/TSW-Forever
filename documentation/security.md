# Security: credentials in logs and repository hygiene

## 1. Credentials in game logs
**Finding.** The original game client writes a line of the form `ChatClient - chat is connected to … and is registering with nick: <name>, password: <value> …` into its own `ClientLog.txt` and to **stdout** when it connects to the chat server. Whether the value is a session token or a lasting password is **not known**; it is treated as a lasting password. The client is not modified, so the launcher can only control what *it* records, shows, copies or passes on.

**Measures in the launcher.**
* Central masking rules: `launcher/src/secrets.rs` (Rust, everything the launcher prints or records) and `scripts/secretmask.py` (Python, test tools). Three layers: (1) **withhold the line** if it looks like credentials, tokens, cookies or names (password in all spellings, `API_KEY`, `client_secret`, `Authorization`, `Bearer`, `loginkey`, IRC `PASS`/`NICK`/`USER`, "registering with nick"); (2) **mask URL user information** in kept lines; the Rust filter does not generally recognise arbitrary e-mail addresses, JWTs or random secret values; (3) **continuation lines** are withheld too when the value follows on the next line.
* Keywords count as words, so the game name (`TheSecretWorldDX11.exe`, `secretworld.com`) does not trigger them.
* Kept: timestamps, DLL loads, Wine/DXVK messages, shader, texture and D3DX errors.
* `tsw-launch` in waiting mode passes the client's stdout **and** stderr line by line through the masker (`launch::run_masked`; the exit code is preserved). `last-launch.txt` masks every field and argument (`-key`, `-loginkey`, `-statindex`, URL credentials). The GUI shows child-process messages only masked. `--log-file` creates a new file with mode `0600`; existing file permissions are not tightened automatically.
* Tests use invented values only (the Rust `secrets`, `launch` and `diag` tests).

**Limits (no complete secrecy).**
1. The client itself writes the secret into `ClientLog.txt` and stdout. Everything the client writes (`ClientLog.txt`, `OldLogs/`, the prefix) must be treated as **private**.
2. A detached start with `--log-file` is written raw (new files use mode `0600`; existing permissions may differ): a filter would need a helper process that outlives the launcher. Without `--log-file` the output is discarded.
3. Terminal output of a client started outside the launcher (own Wine call, Lutris) does not pass the filter.
4. The filter is a line-based heuristic; a secret without keyword or context, wrapped over several lines or binary-encoded can pass. The rules deliberately withhold too much rather than too little.
5. Before sharing any log, look at it first. Screenshots of the login screen show the stored user name.

## 2. Repository hygiene (`scripts/prepublish_check.py`)
The check applies `.gitignore` itself (own matcher, tested) and reports what a commit would contain, without using Git. Blocked: proprietary installers and binaries (suffix **and** magic bytes), game archives, Wine prefix files (`drive_c`, `*.reg`), shader caches, packages and archives, local build/report/log folders, private keys, token and password patterns, the local user's home directory, symlinks and pipes, files above 2 MiB, a total above 25 MiB, images outside the icon folders (one hash-pinned exception for the branding image, see `third-party-components.md`), and any top-level path not on the allowlist (`TOP_ALLOWED`). Files that contain *invented* credentials for tests are listed one by one (`KNOWN_FIXTURES`); private-key blocks and home paths stay forbidden even there. Values are never printed, only paths and line numbers.

* Before a commit: `python3 scripts/prepublish_check.py`; with a repository: `--git` (tracked and untracked files) and once `--history` (history). `--list` prints the candidate files.
* Already tracked but now ignored files are reported; remove them from the index with `git rm --cached -- <path>` (the local file stays). **Never rewrite history without an explicit decision**; if `--history` finds a secret, revoke or rotate it first.
* New file types or folders: first `.gitignore` (ignore or release explicitly), then if necessary `TOP_ALLOWED` or `BRANDING_EXCEPTIONS` with a reason and a test. Never release a whole folder or a size limit in general.
* `scripts/check_secrets_in_tree.py` scans the tree for credential shapes and prints only `path:line`. `scripts/check_release_artifacts.py` audits release artefacts (no Windows programs, no Funcom/Microsoft names). CI runs `prepublish_check.py --git` on every push.

## 3. Updates and downloads
* Flatpak updates: no project update repository is operating. Bundles are updated manually (user-guide.md). A protected, GPG-signed OSTree workflow is prepared but not provisioned/accepted; do not describe its key or signed updates as already deployed. The launcher never overwrites itself.
* Client patches: minisign signature plus SHA-256, atomic replacement, rollback, path and symlink protection, no update while the game runs. Currently inactive (no public key compiled in).
* Downloads (installer, runner, Direct3D components): https only (also after redirects), pinned SHA-256, explicit consent, verification before use (`third-party-components.md`).
* All GitHub Actions are pinned to commit SHAs; policy tests enforce it.

## 4. Reduced diagnostic packages

Logging remains voluntary and off by default, with no telemetry or automatic upload. `diag bundle` serialises a separate configuration representation without saved environment values. It also removes known literal saved values from included old start records, checks and logs before heuristic masking. Common/short values can over-redact the report. Unknown, transformed or encoded values and other personal information may remain. Read the whole package before sharing; no blanket personal-data-free guarantee is made. The game's raw ClientLog and raw CLI `--log-file` are not safe attachments.

## 5. Prefix provenance and public source

`prefix::may_initialise` gates initialisation before invoking Wine or replacing DLLs. Empty prefixes receive a creation marker first; only that pending marker allows an interrupted creation to resume without a manifest. Historical managed prefixes require the complete recognised component manifest. Missing/damaged completed manifests and foreign prefixes, even with user.reg, are refused. This is provenance/integrity policy, not protection from an attacker who can already rewrite all files as the same user.

The private branding exception is not public permission. `public-export.md` describes a separately approved clean source snapshot without private Git history; `check_public_source.py` audits its archive in memory. No public staging directory is created. `LICENSES/inventory.json` and `check_licenses.py` bind required licence texts to the Linux dependency closure and lockfile.
