//! The client's patcher-key check (`-key %08x%08x`), reproduced from the client's own code.
//!
//! The client TEA-decrypts the 64-bit argument with a 128-bit key that is stored in its own executable,
//! requires both decrypted words to be equal, and requires `|time(0) - word| < 120`. A valid key is therefore
//! `TEA_encrypt(t, t)` for the current Unix time `t`. The cipher key is READ FROM THE EXECUTABLE at run time
//! (see [`find_cipher_key`]); it is not embedded in this program.

const DELTA: u32 = 0x9e37_79b9;
const ROUNDS: u32 = 32;
/// How many seconds the client tolerates between the key's time and its own clock (exclusive).
pub const CLIENT_TIME_TOLERANCE_SECS: u32 = 120;

pub type CipherKey = [u32; 4];

pub fn tea_encrypt(mut v0: u32, mut v1: u32, k: &CipherKey) -> (u32, u32) {
    let mut sum = 0u32;
    for _ in 0..ROUNDS {
        sum = sum.wrapping_add(DELTA);
        v0 = v0.wrapping_add((v1 << 4).wrapping_add(k[0]) ^ v1.wrapping_add(sum) ^ (v1 >> 5).wrapping_add(k[1]));
        v1 = v1.wrapping_add((v0 << 4).wrapping_add(k[2]) ^ v0.wrapping_add(sum) ^ (v0 >> 5).wrapping_add(k[3]));
    }
    (v0, v1)
}

pub fn tea_decrypt(mut v0: u32, mut v1: u32, k: &CipherKey) -> (u32, u32) {
    let mut sum = DELTA.wrapping_mul(ROUNDS);
    for _ in 0..ROUNDS {
        v1 = v1.wrapping_sub((v0 << 4).wrapping_add(k[2]) ^ v0.wrapping_add(sum) ^ (v0 >> 5).wrapping_add(k[3]));
        v0 = v0.wrapping_sub((v1 << 4).wrapping_add(k[0]) ^ v1.wrapping_add(sum) ^ (v1 >> 5).wrapping_add(k[1]));
        sum = sum.wrapping_sub(DELTA);
    }
    (v0, v1)
}

/// The `-key` argument value for the given Unix time (seconds, truncated to 32 bits like the client does).
pub fn patcher_key_arg(unix_time: u64, k: &CipherKey) -> String {
    let t = unix_time as u32;
    let (a, b) = tea_encrypt(t, t, k);
    format!("{a:08x}{b:08x}")
}

/// Mirror of the client's acceptance test, used by the self-check and the tests.
pub fn client_would_accept(arg: &str, now: u64, k: &CipherKey) -> bool {
    if arg.len() != 16 {
        return false;
    }
    let (Ok(a), Ok(b)) = (u32::from_str_radix(&arg[..8], 16), u32::from_str_radix(&arg[8..], 16)) else {
        return false;
    };
    let (v0, v1) = tea_decrypt(a, b, k);
    let diff = (now as u32 as i32).wrapping_sub(v1 as i32).unsigned_abs();
    v0 == v1 && diff < CLIENT_TIME_TOLERANCE_SECS
}

const ANCHOR_AFTER: &[u8] = b"Could not decode patcher key";
const ANCHOR_BEFORE: &[u8] = b"Invalid patcher key arg";

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Locate the 128-bit cipher key inside the client executable. In the known builds the 16 key bytes sit between
/// the strings "Invalid patcher key arg" and "Could not decode patcher key" in `.rdata`. Both anchors must be
/// present and in that order, or no key is returned (the launcher then refuses instead of guessing).
pub fn find_cipher_key(exe: &[u8]) -> Option<CipherKey> {
    let after = find(exe, ANCHOR_AFTER)?;
    let before = find(exe, ANCHOR_BEFORE)?;
    // the anchor strings are padded to 4 bytes: [..."arg \0"] [pad] [16 key bytes] ["Could not decode..."]
    if after < 16 || before + ANCHOR_BEFORE.len() > after - 16 || after - 16 - before > 48 {
        return None;
    }
    let raw = &exe[after - 16..after];
    let mut k = [0u32; 4];
    for (i, w) in k.iter_mut().enumerate() {
        *w = u32::from_le_bytes([raw[4 * i], raw[4 * i + 1], raw[4 * i + 2], raw[4 * i + 3]]);
    }
    Some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key found in TheSecretWorldDX11.exe . Test data only.
    const KNOWN: CipherKey = [0x5bce568e, 0xfb2e8cbc, 0xa324e6d8, 0xf22bf865];

    #[test]
    fn matches_values_observed_with_the_real_client() {
        // computed independently (Python) and accepted by the real client in the offline test run
        assert_eq!(patcher_key_arg(1793300058, &KNOWN), "e6ecbe480decdc7b");
        assert_eq!(patcher_key_arg(0xd7ce15eb, &KNOWN), "d175bbe7b80f836a");
    }

    #[test]
    fn roundtrip_and_acceptance_window() {
        for t in [0u64, 1, 1_793_300_058, 0xffff_ffff] {
            let arg = patcher_key_arg(t, &KNOWN);
            assert!(client_would_accept(&arg, t, &KNOWN));
            assert!(client_would_accept(&arg, t.wrapping_add(119), &KNOWN));
            assert!(!client_would_accept(&arg, t.wrapping_add(120), &KNOWN));
            assert!(!client_would_accept(&arg, t.wrapping_sub(120), &KNOWN));
        }
        assert!(!client_would_accept("zzzzzzzzzzzzzzzz", 1, &KNOWN));
        assert!(!client_would_accept("abcd", 1, &KNOWN));
    }

    #[test]
    fn finds_the_key_between_the_anchor_strings_and_refuses_otherwise() {
        let mut buf = b"junk\0%08x%08x\0\0\0\0Invalid patcher key arg \0\0\0".to_vec();
        for w in KNOWN {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        buf.extend_from_slice(b"Could not decode patcher key \0 more");
        assert_eq!(find_cipher_key(&buf), Some(KNOWN));
        assert_eq!(find_cipher_key(b"nothing here"), None);
        let swapped = b"Could not decode patcher key 0123456789abcdefInvalid patcher key arg ".to_vec();
        assert_eq!(find_cipher_key(&swapped), None);
    }
}
