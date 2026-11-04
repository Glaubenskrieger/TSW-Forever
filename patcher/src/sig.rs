//! Manifest signatures: minisign (Ed25519 over the BLAKE2b-512 prehash, signature algorithm `ED`). The public key is compiled into
//! the launcher (or passed in); the private key exists only in the release pipeline. Legacy un-prehashed signatures are refused.

use crate::PatchError;

/// Verify `manifest` against the minisign signature text `sig` with the trusted public key text `public_key`
/// (the base64 line of a `.pub` file, or the whole file).
pub fn verify(manifest: &[u8], sig: &str, public_key: &str) -> Result<(), PatchError> {
    let err = |m: String| PatchError::Signature(m);
    let key_line = public_key.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("untrusted comment:")).unwrap_or("");
    if key_line.is_empty() {
        return Err(err("no trusted public key is configured".into()));
    }
    let pk = minisign_verify::PublicKey::from_base64(key_line).map_err(|e| err(format!("the trusted public key is invalid: {e}")))?;
    let sg = minisign_verify::Signature::decode(sig).map_err(|e| err(format!("the signature file is invalid: {e}")))?;
    pk.verify(manifest, &sg, false).map_err(|e| err(format!("verification failed: {e}")))
}
