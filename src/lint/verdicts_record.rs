//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! One record of a packed verdict file: a magic, the key's and the value's lengths, a checksum of
//! all of them, then the key and the value; and the reading of a file of them.

use sha2::{Digest, Sha256};

/// The bytes every record starts with. `0xff` is no byte of UTF-8, so no key or value holds it.
pub(super) const MAGIC: [u8; 4] = [0xFF, b'v', b'r', b'1'];

/// A record's head: the magic, the key's and the value's lengths, and the checksum.
pub(super) const HEAD: usize = MAGIC.len() + 4 + 4 + 16;

/// The checksum of a record: the key's and the value's lengths, then both.
fn checksum(key: &[u8], value: &[u8]) -> [u8; 16] {
    let mut h = Sha256::new();
    h.update((key.len() as u32).to_le_bytes());
    h.update((value.len() as u32).to_le_bytes());
    h.update(key);
    h.update(value);
    let full: [u8; 32] = h.finalize().into();
    let mut out = [0; 16];
    out.copy_from_slice(&full[.. 16]);
    out
}

/// One record, or `None` for a key or value too long for its length field.
pub(super) fn encode(key: &str, value: &str) -> Option<Vec<u8>> {
    let (k, v) = (key.as_bytes(), value.as_bytes());
    let mut out = Vec::with_capacity(HEAD + k.len() + v.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&u32::try_from(k.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&u32::try_from(v.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&checksum(k, v));
    out.extend_from_slice(k);
    out.extend_from_slice(v);
    Some(out)
}

/// The record starting at `at`, and where it ends, when it is whole and its checksum holds.
fn record_at(bytes: &[u8], at: usize) -> Option<(String, String, usize)> {
    let head = bytes.get(at .. at.checked_add(HEAD)?)?;
    let len = |r: std::ops::Range<usize>| {
        Some(u32::from_le_bytes(head.get(r)?.try_into().ok()?) as usize)
    };
    let (klen, vlen) = (len(4 .. 8)?, len(8 .. 12)?);
    let kend = (at + HEAD).checked_add(klen)?;
    let end = kend.checked_add(vlen)?;
    let (k, v) = (bytes.get(at + HEAD .. kend)?, bytes.get(kend .. end)?);
    if head[12 ..] != checksum(k, v) {
        return None;
    }
    Some((
        String::from_utf8(k.to_vec()).ok()?,
        String::from_utf8(v.to_vec()).ok()?,
        end,
    ))
}

/// Every record of a packed file whose checksum holds, in order. Past one that does not, the
/// reading goes on from the next magic.
pub(super) fn parse(bytes: &[u8]) -> Vec<(String, String)> {
    let next_magic = |from: usize| {
        bytes
            .get(from ..)
            .and_then(|rest| rest.windows(MAGIC.len()).position(|w| w == MAGIC))
            .map(|p| p + from)
    };
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = next_magic(at) {
        match record_at(bytes, start) {
            Some((key, value, end)) => {
                out.push((key, value));
                at = end;
            },
            None => at = start + 1,
        }
    }
    out
}
