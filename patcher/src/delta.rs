//! A small binary delta (copy/insert) so that a changed client file costs a few kilobytes instead of a full replacement.
//! Format `TSWD1`: magic, old length (u64 LE), new length (u64 LE), then operations: `0` + offset + length (copy from the old file)
//! or `1` + length + bytes (insert). Applying checks every bound; the caller verifies the SHA-256 of the result.

use std::collections::HashMap;

use crate::PatchError;

const MAGIC: &[u8; 5] = b"TSWD1";
const BLOCK: usize = 32;
/// Largest file a delta will produce or consume (memory-based implementation).
pub const MAX_FILE: u64 = 1 << 30;

fn rolling_base() -> u64 {
    0x100000001b3
}

fn hash_block(b: &[u8]) -> u64 {
    b.iter().fold(0u64, |h, &x| h.wrapping_mul(rolling_base()).wrapping_add(x as u64 + 1))
}

/// Create a delta that turns `old` into `new`.
pub fn create(old: &[u8], new: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + new.len() / 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(old.len() as u64).to_le_bytes());
    out.extend_from_slice(&(new.len() as u64).to_le_bytes());
    let mut index: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut i = 0;
    while i + BLOCK <= old.len() {
        index.entry(hash_block(&old[i..i + BLOCK])).or_default().push(i);
        i += BLOCK;
    }
    let mut pending: Vec<u8> = Vec::new();
    let flush = |out: &mut Vec<u8>, pending: &mut Vec<u8>| {
        if !pending.is_empty() {
            out.push(1);
            out.extend_from_slice(&(pending.len() as u64).to_le_bytes());
            out.extend_from_slice(pending);
            pending.clear();
        }
    };
    let top = rolling_base().wrapping_pow(BLOCK as u32 - 1);
    let (mut pos, mut h) = (0usize, if new.len() >= BLOCK { hash_block(&new[..BLOCK]) } else { 0 });
    while pos < new.len() {
        let mut matched: Option<(usize, usize)> = None;
        if pos + BLOCK <= new.len() {
            if let Some(cands) = index.get(&h) {
                for &o in cands {
                    if old[o..o + BLOCK] == new[pos..pos + BLOCK] {
                        let mut len = BLOCK;
                        while o + len < old.len() && pos + len < new.len() && old[o + len] == new[pos + len] {
                            len += 1;
                        }
                        if matched.is_none_or(|(_, l)| len > l) {
                            matched = Some((o, len));
                        }
                    }
                }
            }
        }
        if let Some((o, len)) = matched {
            flush(&mut out, &mut pending);
            out.push(0);
            out.extend_from_slice(&(o as u64).to_le_bytes());
            out.extend_from_slice(&(len as u64).to_le_bytes());
            pos += len;
            if pos + BLOCK <= new.len() {
                h = hash_block(&new[pos..pos + BLOCK]);
            }
        } else {
            pending.push(new[pos]);
            if pos + BLOCK < new.len() {
                // roll: drop new[pos], add new[pos + BLOCK]
                h = h.wrapping_sub((new[pos] as u64 + 1).wrapping_mul(top)).wrapping_mul(rolling_base()).wrapping_add(new[pos + BLOCK] as u64 + 1);
            }
            pos += 1;
        }
    }
    flush(&mut out, &mut pending);
    out
}

fn u64_at(d: &[u8], at: usize) -> Result<u64, PatchError> {
    d.get(at..at + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| PatchError::Delta("truncated".into()))
}

/// Apply `delta` to `old`. Every offset and length is checked; the output never exceeds the declared size.
pub fn apply(old: &[u8], delta: &[u8]) -> Result<Vec<u8>, PatchError> {
    let bad = |m: &str| Err(PatchError::Delta(m.to_string()));
    if delta.len() < 21 || &delta[..5] != MAGIC {
        return bad("not a TSWD1 delta");
    }
    let (old_len, new_len) = (u64_at(delta, 5)?, u64_at(delta, 13)?);
    if old_len != old.len() as u64 {
        return bad("the delta was made for a different base file (length differs)");
    }
    if new_len > MAX_FILE {
        return bad("declared result too large");
    }
    let mut out = Vec::with_capacity(new_len as usize);
    let mut at = 21;
    while at < delta.len() {
        match delta[at] {
            0 => {
                let (off, len) = (u64_at(delta, at + 1)?, u64_at(delta, at + 9)?);
                let end = off.checked_add(len).filter(|e| *e <= old.len() as u64);
                let Some(end) = end else {
                    return bad("copy outside the base file");
                };
                if out.len() as u64 + len > new_len {
                    return bad("result longer than declared");
                }
                out.extend_from_slice(&old[off as usize..end as usize]);
                at += 17;
            }
            1 => {
                let len = u64_at(delta, at + 1)?;
                let end = (at as u64 + 9).checked_add(len).filter(|e| *e <= delta.len() as u64);
                let Some(end) = end else {
                    return bad("insert beyond the end of the delta");
                };
                if out.len() as u64 + len > new_len {
                    return bad("result longer than declared");
                }
                out.extend_from_slice(&delta[at + 9..end as usize]);
                at = end as usize;
            }
            _ => return bad("unknown operation"),
        }
    }
    if out.len() as u64 != new_len {
        return bad("result shorter than declared");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn round_trips_edits_inserts_deletions_and_unrelated_files() {
        let old = data(100_000, 1);
        let mut edited = old.clone();
        edited[5_000..5_010].copy_from_slice(b"PATCHED!!!");
        let mut inserted = old.clone();
        inserted.splice(40_000..40_000, data(777, 9));
        let mut deleted = old.clone();
        deleted.drain(10_000..30_000);
        for (name, new) in [("edit", edited), ("insert", inserted), ("delete", deleted), ("unrelated", data(5_000, 7)), ("empty", vec![]), ("same", old.clone())] {
            let d = create(&old, &new);
            assert_eq!(apply(&old, &d).unwrap(), new, "{name}");
            if name != "unrelated" && name != "empty" {
                assert!(d.len() < old.len() / 5, "{name}: the delta is small ({} bytes)", d.len());
            }
        }
        assert_eq!(apply(&[], &create(&[], b"hello")).unwrap(), b"hello");
        assert_eq!(apply(&[], &create(&[], &[])).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn corrupt_or_mismatched_deltas_are_rejected_without_panicking() {
        let old = data(4_000, 3);
        let new = data(4_100, 4);
        let d = create(&old, &new);
        assert!(apply(&data(4_001, 3), &d).is_err(), "wrong base length");
        assert!(apply(&old, &d[..d.len() - 3]).is_err(), "truncated");
        assert!(apply(&old, b"nonsense").is_err());
        let mut bad = d.clone();
        bad[21] = 7;
        assert!(apply(&old, &bad).is_err(), "unknown op");
        let mut oob = create(&old, &old);
        oob[22..30].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(apply(&old, &oob).is_err(), "copy outside / overflow");
        let mut huge = d.clone();
        huge[13..21].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(apply(&old, &huge).is_err(), "absurd declared size");
        for cut in 0..d.len().min(60) {
            let _ = apply(&old, &d[..cut]);
        }
    }
}
