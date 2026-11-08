//! A fingerprint of a document's text, so copies of one passage are counted once.
//!
//! Research here is copied between projects whole and then edited, so the same argument turns up
//! in five places and every count and ranking sees five answers where there is one. A document is
//! reduced to the hashes of its word fivegrams, sampled by a fixed rule so two copies sample the
//! same fivegrams, and two documents are copies when the smaller one's sample sits inside the
//! larger one's.

/// How many words one shingle spans.
const SPAN: usize = 5;

/// One shingle in this many is kept. A fixed residue, not a random draw, so a copy keeps what its
/// original kept.
const SAMPLE_EVERY: u64 = 8;

/// A sample smaller than this says nothing about the document it came from.
const MIN_SAMPLE: usize = 8;

/// The containment at or above which the smaller document is a copy of the larger.
pub const COPY_AT: f64 = 0.8;

/// FNV-1a over bytes: fixed forever, unlike the standard hasher, so a cache written yesterday is
/// readable today.
fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The lowercased alphanumeric words of a text.
pub fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

/// The sorted, deduplicated sample of a text's shingle hashes.
#[must_use]
pub fn sketch(text: &str) -> Vec<u64> {
    let ws: Vec<String> = words(text).collect();
    if ws.len() < SPAN {
        return Vec::new();
    }
    let mut out: Vec<u64> = ws
        .windows(SPAN)
        .map(|w| fnv(w.join(" ").as_bytes()))
        .filter(|h| h % SAMPLE_EVERY == 0)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// How much of the smaller sample the larger one holds, or zero where either is too small to say.
#[must_use]
pub fn containment(a: &[u64], b: &[u64]) -> f64 {
    if a.len() < MIN_SAMPLE || b.len() < MIN_SAMPLE {
        return 0.0;
    }
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let inside = small
        .iter()
        .filter(|h| large.binary_search(h).is_ok())
        .count();
    inside as f64 / small.len() as f64
}

/// A sketch as the bytes stored in the index.
#[must_use]
pub fn to_bytes(sketch: &[u64]) -> Vec<u8> {
    sketch.iter().flat_map(|h| h.to_le_bytes()).collect()
}

/// The inverse of [`to_bytes`]. A trailing partial word is dropped, not guessed at.
#[must_use]
pub fn from_bytes(bytes: &[u8]) -> Vec<u64> {
    bytes
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap_or([0; 8])))
        .collect()
}

#[cfg(test)]
mod tests;
