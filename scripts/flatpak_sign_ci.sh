#!/usr/bin/env bash
# Signs the STABLE Flatpak repository inside the release pipeline (job `sign` of .github/workflows/release.yml). The only secrets it ever sees are the signing SUBKEY of the project
# key and its passphrase, handed over by the protected GitHub environment `flatpak-release` after the owner's approval; the PRIMARY key stays with the owner for good.
#
#   scripts/flatpak_sign_ci.sh <repository> <public-key-file>
#
#   TSW_FLATPAK_SIGNING_KEY          (secret)   the signing subkey, base64 on ONE line (armored `gpg --export-secret-subkeys`, primary secret key only a stub)
#   TSW_FLATPAK_SIGNING_PASSPHRASE   (secret)   the passphrase of the subkey (empty only for a key without passphrase: automated tests)
#   TSW_FLATPAK_SIGNING_FINGERPRINT  (variable) the full 40-digit fingerprint of the PRIMARY key
#   TSW_FLATPAK_SIGNING_SUBKEY_ID    (variable) the 16-digit key id of the signing subkey
#   TSW_FLATPAK_SIGNING_PIN_FILE     optional   a file with FLATPAK_SIGNING_FINGERPRINT= and FLATPAK_SIGNING_SUBKEY_ID= (default packaging/project.conf): the variables must equal the values
#                                               pinned in the repository (automated tests point it at a throw-away file or /dev/null)
#
# What it does: imports the subkey into a throw-away GNUPGHOME (mode 700, outside the repository, removed at the end), refuses a secret that carries the PRIMARY secret key or anything
# but exactly one signing subkey, a primary fingerprint or subkey id that differs from the variables (and from the values pinned in the repository), a committed public key that does not
# contain exactly this primary key and subkey, and a passphrase that does not unlock the subkey. The passphrase is given to the throw-away gpg-agent (gpg-preset-passphrase, read from a
# pipe) so that Flatpak's own gpg call can sign without any prompt; it is unset right after. Then it signs every app commit and the summary with exactly that subkey (static deltas are
# generated AFTER the signing, flatpak_keys.sh sign-repo), verifies the result against the committed public key the way a client does (flatpak_keys.sh verify-repo), proves the update over
# the signed static deltas (check_flatpak_delta_update.sh --require-delta) and scans the repository for key material. Output: fingerprints and counts only, never key material or the passphrase.
# Hard rules: no `set -x` (refused), nothing secret in an argument or in a file, the keyring and the agent are removed on every exit path.
# Exit codes: 0 signed and verified, 1 a check failed, 2 wrong call or missing input, 3 unsafe location, 10 a tool is missing.
set -euo pipefail
export LC_ALL=C
case "$-" in *x*) echo "flatpak_sign_ci: refusing to run with tracing (set -x) enabled: it could print the key" >&2; exit 2;; esac
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
die() { echo "flatpak_sign_ci: $*" >&2; exit "${2:-1}"; }
repo="${1:?repository}"; pub="${2:?public key file}"
for t in gpg gpg-agent gpgconf ostree flatpak base64 python3; do command -v "$t" >/dev/null 2>&1 || die "missing tool $t" 10; done
preset="$(gpgconf --list-dirs libexecdir 2>/dev/null)/gpg-preset-passphrase"
[ -x "$preset" ] || die "gpg-preset-passphrase is missing ($preset): install the package gpg-agent" 10
[ -n "${TSW_FLATPAK_SIGNING_KEY:-}" ] || die "TSW_FLATPAK_SIGNING_KEY is not set (it comes from the protected environment flatpak-release)" 2
fpr="${TSW_FLATPAK_SIGNING_FINGERPRINT:-}"; subid="${TSW_FLATPAK_SIGNING_SUBKEY_ID:-}"
[[ "$fpr" =~ ^[0-9A-Fa-f]{40}$ ]] || die "TSW_FLATPAK_SIGNING_FINGERPRINT must be the full 40 hex digit fingerprint of the primary key" 2
[[ "$subid" =~ ^[0-9A-Fa-f]{16}$ ]] || die "TSW_FLATPAK_SIGNING_SUBKEY_ID must be the 16 hex digit key id of the signing subkey" 2
fpr="${fpr^^}"; subid="${subid^^}"
[ -d "$repo/objects" ] || die "$repo is not an OSTree repository" 2
[ -s "$pub" ] || die "the public key file is missing or empty: $pub" 2
packets="$(gpg --list-packets "$pub" 2>/dev/null || true)"
if grep -Eq ':secret (sub )?key packet' <<<"$packets"; then die "$pub holds a SECRET key: only public key parts belong in the repository" 1; fi
grep -q ':public key packet' <<<"$packets" || die "$pub holds no public key" 1

# the values pinned in the repository (packaging/project.conf) must be the values of the variables: a changed variable cannot switch the key
pinfile="${TSW_FLATPAK_SIGNING_PIN_FILE-$root/packaging/project.conf}"
if [ -n "$pinfile" ] && [ -r "$pinfile" ]; then
  pinf="$(sed -n 's/^FLATPAK_SIGNING_FINGERPRINT=//p' "$pinfile" | head -1 | tr -d '"' | tr a-f A-F)"; pins="$(sed -n 's/^FLATPAK_SIGNING_SUBKEY_ID=//p' "$pinfile" | head -1 | tr -d '"' | tr a-f A-F)"
  if [ -n "$pinf" ] && [ "$pinf" != "$fpr" ]; then die "the fingerprint variable ($fpr) is not the fingerprint pinned in $(basename "$pinfile") ($pinf)"; fi
  if [ -n "$pins" ] && [ "$pins" != "$subid" ]; then die "the subkey id variable ($subid) is not the subkey id pinned in $(basename "$pinfile") ($pins)"; fi
fi

home="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/tsw-sign.XXXXXXXX")"
chmod 700 "$home"
case "$(realpath -m "$home")/" in "$root"/*) rm -rf "${home:?}"; die "the keyring would be inside the repository" 3;; esac
cleanup() { GNUPGHOME="$home" gpgconf --kill gpg-agent >/dev/null 2>&1 || true; rm -rf "${home:?}"; }
trap cleanup EXIT
export GNUPGHOME="$home"
printf 'allow-preset-passphrase\ndefault-cache-ttl 7200\nmax-cache-ttl 7200\n' > "$home/gpg-agent.conf"

# the secret goes through a pipe from a builtin: it is in no argument list and in no file
if ! printf '%s' "$TSW_FLATPAK_SIGNING_KEY" | base64 -d 2>/dev/null | gpg --batch --quiet --import >/dev/null 2>&1; then
  unset TSW_FLATPAK_SIGNING_KEY TSW_FLATPAK_SIGNING_PASSPHRASE; die "TSW_FLATPAK_SIGNING_KEY cannot be imported (it must be the single-line base64 of an armored secret subkey)"
fi
unset TSW_FLATPAK_SIGNING_KEY

# the primary key must only be a stub (sec#); exactly one secret subkey, able to sign only
sk="$(gpg --list-secret-keys 2>/dev/null || true)"; skc="$(gpg --list-secret-keys --with-colons --with-keygrip 2>/dev/null || true)"
[ -n "$sk" ] || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the imported secret holds no key"; }
grep -q '^sec#' <<<"$sk" || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the imported secret contains the PRIMARY secret key: refused. Only the signing subkey belongs in CI"; }
if grep -q '^sec ' <<<"$sk"; then unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the imported secret contains a full secret key besides the stub: refused"; fi
imported="$(awk -F: '$1=="sec"{f=1} f&&$1=="fpr"{print toupper($10); exit}' <<<"$skc")"
[ "$imported" = "$fpr" ] || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the imported primary key ($imported) is not the key named in TSW_FLATPAK_SIGNING_FINGERPRINT ($fpr)"; }
nsub="$(awk -F: '$1=="ssb"{n++} END{print n+0}' <<<"$skc")"
[ "$nsub" -eq 1 ] || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the imported secret must hold exactly one signing subkey (found $nsub)"; }
subcap="$(awk -F: '$1=="ssb"{print $12; exit}' <<<"$skc")"
case "${subcap//[A-Z]/}" in s) ;; *) unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the imported subkey can do more than sign (capabilities $subcap)";; esac
subfpr="$(awk -F: '$1=="ssb"{f=1;next} f&&$1=="fpr"{print toupper($10); exit}' <<<"$skc")"
[ "${subfpr: -16}" = "$subid" ] || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the imported subkey (${subfpr: -16}) is not the key named in TSW_FLATPAK_SIGNING_SUBKEY_ID ($subid)"; }
grip="$(awk -F: '$1=="ssb"{f=1;next} f&&$1=="grp"{print $10; exit}' <<<"$skc")"
[ -n "$grip" ] || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the keygrip of the subkey is unknown"; }

# the committed public key must be the public half of exactly this key: the primary key and this subkey
pubc="$(gpg --show-keys --with-colons "$pub" 2>/dev/null || true)"
pubprimary="$(awk -F: '$1=="pub"{f=1} f&&$1=="fpr"{print toupper($10); exit}' <<<"$pubc")"
pubsubs="$(awk -F: '$1=="sub"{f=1;next} f&&$1=="fpr"{print toupper($10); f=0}' <<<"$pubc")"
[ "$(grep -c '^pub:' <<<"$pubc")" -eq 1 ] || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the committed public key file must hold exactly one key"; }
[ "$pubprimary" = "$fpr" ] || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the committed public key ($pubprimary) is not the key named in TSW_FLATPAK_SIGNING_FINGERPRINT ($fpr)"; }
grep -qx "$subfpr" <<<"$pubsubs" || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the signing subkey $subid is not part of the committed public key (was it added after the public key was exported?)"; }

# the passphrase goes to the throw-away agent through a pipe; Flatpak's own gpg call then signs without a prompt
gpg-connect-agent /bye >/dev/null 2>&1 || die "the gpg agent does not start"
if [ -n "${TSW_FLATPAK_SIGNING_PASSPHRASE:-}" ]; then
  printf '%s' "$TSW_FLATPAK_SIGNING_PASSPHRASE" | "$preset" --preset "$grip" >/dev/null 2>&1 || { unset TSW_FLATPAK_SIGNING_PASSPHRASE; die "the passphrase could not be handed to the gpg agent"; }
fi
unset TSW_FLATPAK_SIGNING_PASSPHRASE
probe="$(mktemp "$home/probe.XXXXXX")"; echo probe > "$probe"
gpg --batch --pinentry-mode error --local-user "${subid}!" --detach-sign --output "$probe.sig" "$probe" >/dev/null 2>&1 || die "the signing subkey cannot sign: a wrong or missing passphrase (TSW_FLATPAK_SIGNING_PASSPHRASE)"
rm -f "$probe" "$probe.sig"
echo "signing key: primary $fpr, subkey $subid (signing only), primary secret key not present, subkey unlocked"

"$root/scripts/flatpak_keys.sh" sign-repo "$home" "${subid}!" "$repo"
"$root/scripts/flatpak_keys.sh" verify-repo "$pub" "$repo"
# a delta generated before the signing carries no signature: prove the update itself with the committed public key only
"$root/scripts/check_flatpak_delta_update.sh" --require-delta "$repo" "$pub"

# no key material may be left in what is handed on
leak="$(grep -rIl --binary-files=without-match -e 'BEGIN PGP PRIVATE KEY' -e 'PRIVATE KEY BLOCK' "$repo" 2>/dev/null || true)"
[ -z "$leak" ] || die "key material found in the repository: refused"
echo "signed and verified: $repo"
