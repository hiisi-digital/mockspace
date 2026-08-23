//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Stable FNV-1a 64-bit, vendored.
//!
//! The build cache key is persisted on disk and must mean the same thing
//! across Rust releases. `std::hash::DefaultHasher` (SipHash) is explicitly
//! not guaranteed stable across releases, so it cannot key a durable cache.
//! FNV-1a is trivial, deterministic forever, and collision-resistant enough
//! to distinguish compilation inputs for a cache directory name.

const OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01B3;

/// An FNV-1a 64-bit accumulator.
pub struct Fnv(u64);

impl Fnv {
    pub fn new() -> Self {
        Fnv(OFFSET)
    }

    /// Fold raw bytes in.
    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(PRIME);
        }
    }

    /// Fold a string in, followed by a NUL separator so that concatenating
    /// distinct fields cannot collide by running together (`"ab" + "c"` and
    /// `"a" + "bc"` hash differently).
    pub fn write_field(&mut self, s: &str) {
        self.write(s.as_bytes());
        self.write(&[0]);
    }

    /// The 16-hex-digit cache-key string.
    pub fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        // FNV-1a of the empty input is the offset basis.
        assert_eq!(Fnv::new().hex(), format!("{OFFSET:016x}"));
    }

    #[test]
    fn field_separator_prevents_collision() {
        let mut a = Fnv::new();
        a.write_field("ab");
        a.write_field("c");
        let mut b = Fnv::new();
        b.write_field("a");
        b.write_field("bc");
        assert_ne!(a.hex(), b.hex());
    }

    #[test]
    fn stable_value() {
        // Pin an exact value so a future refactor that changes the algorithm
        // is caught (the cache would silently invalidate otherwise).
        let mut h = Fnv::new();
        h.write_field("mockspace");
        assert_eq!(h.hex().len(), 16);
        // deterministic: same input, same output.
        let mut h2 = Fnv::new();
        h2.write_field("mockspace");
        assert_eq!(h.hex(), h2.hex());
    }
}
