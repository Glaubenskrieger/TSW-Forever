#!/usr/bin/env bash
# Key tooling for the STABLE Flatpak repository (GPG). The production keys already exist and are never created by this repository: a certify-only PRIMARY key (RSA 4096, no expiry,
# fingerprint pinned in packaging/project.conf: FAF3884F3E41D5C63FE656DC86075D3D53A6484B) that stays with the owner for good, and a separate SIGNING SUBKEY (RSA 4096, sign only, no expiry,
# key id 43F56AAA62FBF7DA). The release pipeline signs inside GitHub Actions with that subkey only (scripts/flatpak_sign_ci.sh); no private key and no passphrase ever enters the repository, a log
# or an artifact. The commands below that create or export keys are for AUTOMATED TESTS with throw-away keys; `inspect-ci-export`, `check-pin`, `export-public`, `fingerprints` and
# `verify-repo` are for the owner and the pipeline.
#
#   scripts/flatpak_keys.sh generate-split <gnupg-home> "<user id>" [primary-expiry] [subkey-expiry]
#                                                                         FOR AUTOMATED TESTS (throw-away keys): the production model - a certify-only PRIMARY key and a
#                                                                         separate SIGNING SUBKEY. The production keys already exist (created and backed up by the owner)
#                                                                         and are never created by this repository. Test options: TSW_TEST_KEY_ALGO (default rsa4096 = the production algorithm; the other tests use ed25519 for speed),
#                                                                         TSW_TEST_KEY_PASSPHRASE (protect both keys, like the production keys)
#   scripts/flatpak_keys.sh export-ci-subkey <gnupg-home> <primary-fpr> <file>
#                                                                         FOR AUTOMATED TESTS: write the secret SUBKEY only (no primary secret key), base64 on ONE line
#                                                                         (TSW_TEST_KEY_PASSPHRASE unlocks a protected test key). The owner exports the production subkey
#                                                                         by hand: documentation/release-pipeline.md section 3
#   scripts/flatpak_keys.sh inspect-ci-export <file>                      check a base64 export BEFORE it is uploaded: imports it into a throw-away keyring and prints the
#                                                                         primary fingerprint, whether the primary secret key is only a stub, and the secret subkey ids;
#                                                                         exits 1 if the file holds the primary secret key or anything but signing subkeys. Prints no secret.
#   scripts/flatpak_keys.sh check-pin <public-key-file> <project.conf>    the committed public key holds ONLY public parts, exactly one key, whose primary fingerprint and signing
#                                                                         subkey id are the values pinned in project.conf (FLATPAK_SIGNING_FINGERPRINT, FLATPAK_SIGNING_SUBKEY_ID)
#   scripts/flatpak_keys.sh fingerprints <public-key-file>                print the primary and subkey fingerprints (a subkey's long key id = the last 16 digits)
#   scripts/flatpak_keys.sh export-public <gnupg-home> <key> <file>       write the PUBLIC key (binary) for the bundles and the .flatpakrepo, normally
#                                                                         packaging/flatpak/stable-repo.gpg; refuses to export a secret key
#   scripts/flatpak_keys.sh sign-repo <gnupg-home> <key> <repo>           sign an OSTree repository (the pipeline passes the signing subkey id, <id>!; tests pass a throw-away keyring): every app commit and the summary (+ static deltas)
#   scripts/flatpak_keys.sh verify-repo <public-key-file> <repo>          check that commits and summary verify against the public key, as a user's Flatpak would;
#                                                                         exits 1 for an unsigned or wrongly signed repository (also used by the release workflow)
#
# See documentation/release-pipeline.md (key model, providing the subkey, GitHub settings) and documentation/packaging.md (rotation and revocation).
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
die() { echo "flatpak_keys: $*" >&2; exit "${2:-1}"; }
need() { command -v "$1" >/dev/null 2>&1 || die "missing tool '$1'" 10; }
outside_repo() { case "$(realpath -m "$1")/" in "$root"/*) die "the keyring $1 is inside the repository: use a folder outside of it" 3;; esac; }

cmd="${1:-}"; shift || true
case "$cmd" in
  generate-split)
    home="${1:?gnupg home}"; uid="${2:?user id}"; pexp="${3:-never}"; sexp="${4:-never}"; algo="${TSW_TEST_KEY_ALGO:-rsa4096}"
    need gpg; outside_repo "$home"
    [ ! -e "$home" ] || die "$home exists already: a new key needs a NEW keyring" 4
    mkdir -m 700 -p "$home"
    pw="${TSW_TEST_KEY_PASSPHRASE:-}"
    GNUPGHOME="$home" gpg --batch --pinentry-mode loopback --passphrase "$pw" --quick-gen-key "$uid" "$algo" cert "$pexp" >/dev/null 2>&1 || die "could not create the primary key"
    fpr="$(GNUPGHOME="$home" gpg --list-keys --with-colons | awk -F: '/^fpr/{print $10; exit}')"
    GNUPGHOME="$home" gpg --batch --pinentry-mode loopback --passphrase "$pw" --quick-add-key "$fpr" "$algo" sign "$sexp" >/dev/null 2>&1 || die "could not add the signing subkey"
    echo "$fpr"
    echo "test keyring: $home (throw-away keys for automated tests only)" >&2;;
  export-ci-subkey)
    home="${1:?gnupg home}"; key="${2:?primary fingerprint}"; file="${3:?output file}"
    need gpg; need base64; outside_repo "$file"
    umask 077
    pf="$(mktemp)"; trap 'rm -f "$pf"' EXIT; printf '%s' "${TSW_TEST_KEY_PASSPHRASE:-}" > "$pf"
    GNUPGHOME="$home" gpg --batch --pinentry-mode loopback --passphrase-file "$pf" --armor --export-secret-subkeys "$key" 2>/dev/null | base64 -w0 > "$file"
    rm -f "$pf"
    [ -s "$file" ] || { rm -f "$file"; die "nothing exported: wrong key id or passphrase"; }
    "$0" inspect-ci-export "$file" >/dev/null || { rm -f "$file"; die "the export is not a signing-subkey-only export: refused and removed"; }
    echo "written $file (one line, base64; primary secret key NOT included); paste it into the environment secret, then delete the file";;
  inspect-ci-export)
    file="${1:?exported file}"; need gpg; need base64
    [ -s "$file" ] || die "the file is missing or empty"
    chk="$(mktemp -d)"; chmod 700 "$chk"; trap 'GNUPGHOME="$chk" gpgconf --kill gpg-agent >/dev/null 2>&1; rm -rf "${chk:?}"' EXIT
    base64 -d "$file" 2>/dev/null | GNUPGHOME="$chk" gpg --batch --import >/dev/null 2>&1 || die "the file cannot be imported (it must be the single-line base64 of an armored secret subkey export)"
    listing="$(GNUPGHOME="$chk" gpg --list-secret-keys 2>/dev/null)"; colons="$(GNUPGHOME="$chk" gpg --list-secret-keys --with-colons 2>/dev/null)"
    pfpr="$(awk -F: '$1=="sec"{f=1} f&&$1=="fpr"{print toupper($10); exit}' <<<"$colons")"
    grep -q '^sec#' <<<"$listing" || die "the file holds the PRIMARY secret key (the primary key must be only a stub, sec#): do not upload it"
    grep -q '^sec ' <<<"$listing" && die "the file holds a full secret key: do not upload it"
    n=0
    while IFS=: read -r kind _ _ _ id _ _ _ _ _ _ cap _; do
      if [ "$kind" = ssb ]; then
        n=$((n+1))
        case "${cap//[A-Z]/}" in s) ;; *) die "a secret subkey ($id) can do more than sign (capabilities: $cap): export the signing subkey only";; esac
        echo "secret subkey id: $id"
      fi
    done <<<"$colons"
    [ "$n" -eq 1 ] || die "the file must hold exactly one secret subkey (found $n)"
    echo "primary fingerprint: $pfpr"
    echo "primary secret key: stub only (good)"
    echo "OK: a single signing subkey, no primary secret key";;
  check-pin)
    pubfile="${1:?public key file}"; conf="${2:?project.conf}"; need gpg
    [ -s "$pubfile" ] || die "the public key file is missing or empty: $pubfile"
    pinf="$(sed -n 's/^FLATPAK_SIGNING_FINGERPRINT=//p' "$conf" | head -1 | tr -d '"' | tr a-f A-F)"; pins="$(sed -n 's/^FLATPAK_SIGNING_SUBKEY_ID=//p' "$conf" | head -1 | tr -d '"' | tr a-f A-F)"
    [[ "$pinf" =~ ^[0-9A-F]{40}$ && "$pins" =~ ^[0-9A-F]{16}$ ]] || die "$conf does not pin FLATPAK_SIGNING_FINGERPRINT (40 hex digits) and FLATPAK_SIGNING_SUBKEY_ID (16 hex digits)"
    packets="$(gpg --list-packets "$pubfile" 2>/dev/null || true)"
    grep -Eq ':secret (sub )?key packet' <<<"$packets" && die "$pubfile holds SECRET key parts: only public key parts belong in the repository"
    colons="$(gpg --show-keys --with-colons "$pubfile" 2>/dev/null || true)"
    [ "$(grep -c '^pub:' <<<"$colons")" -eq 1 ] || die "$pubfile must hold exactly one key"
    prim="$(awk -F: '$1=="pub"{f=1} f&&$1=="fpr"{print toupper($10); exit}' <<<"$colons")"
    [ "$prim" = "$pinf" ] || die "the primary fingerprint of $pubfile ($prim) is not the pinned one ($pinf)"
    subs="$(awk -F: '$1=="sub"{f=1;next} f&&$1=="fpr"{print toupper($10); f=0}' <<<"$colons")"
    grep -q "${pins}\$" <<<"$subs" || die "$pubfile has no signing subkey with the pinned id $pins"
    echo "public key file OK: primary $prim, signing subkey $pins, public parts only";;
  fingerprints)
    pubfile="${1:?public key file}"; need gpg
    gpg --show-keys --with-colons "$pubfile" 2>/dev/null | awk -F: '$1=="pub"{k="primary"} $1=="sub"{k="subkey"} $1=="fpr"{print k" "$10}'
    ;;
  export-public)
    home="${1:?gnupg home}"; key="${2:?key}"; file="${3:?output file}"
    need gpg
    GNUPGHOME="$home" gpg --export "$key" > "$file"
    [ -s "$file" ] || die "nothing exported: wrong key id?"
    # a public export must not contain a secret key packet
    if GNUPGHOME="$home" gpg --list-packets "$file" | grep -qE '^:secret'; then rm -f "$file"; die "the export contains a secret key; removed" 5; fi
    echo "public key written to $file";;
  sign-repo)
    home="${1:?gnupg home}"; key="${2:?key}"; repo="${3:?repository}"
    need flatpak; need gpg; need ostree; outside_repo "$home"
    [ -d "$repo/objects" ] || die "$repo is not an OSTree repository"
    while read -r ref; do
      case "$ref" in
        app/*/*/*) id="${ref#app/}"; id="${id%%/*}"; branch="${ref##*/}"
                   flatpak build-sign --gpg-sign="$key" --gpg-homedir="$home" "$repo" "$id" "$branch";;
      esac
    done < <(ostree refs --repo="$repo")
    flatpak build-update-repo --gpg-sign="$key" --gpg-homedir="$home" --generate-static-deltas --prune --prune-depth=2 "$repo" >/dev/null
    echo "signed: $repo";;
  verify-repo)
    pub="${1:?public key file}"; repo="${2:?repository}"
    need ostree; [ -s "$pub" ] || die "public key file is empty or missing"
    [ -d "$repo/objects" ] || die "$repo is not an OSTree repository"
    tmp="$(mktemp -d)"; trap 'rm -rf "${tmp:?}"' EXIT
    ostree --repo="$tmp/r" init --mode=bare-user-only
    ostree --repo="$tmp/r" remote add --gpg-import="$pub" check "file://$(realpath "$repo")"
    n=0
    while read -r ref; do
      case "$ref" in
        app/*) ostree --repo="$tmp/r" pull check "$ref" >/dev/null 2>"$tmp/err" || { cat "$tmp/err" >&2; die "$ref does not verify against the public key (unsigned or signed with another key)"; }
               n=$((n+1));;
      esac
    done < <(ostree refs --repo="$repo")
    [ "$n" -ge 1 ] || die "the repository has no app ref"
    echo "verified: $n ref(s) have valid signatures of the given public key";;
  *) sed -n '2,15p' "${BASH_SOURCE[0]}"; exit 2;;
esac
