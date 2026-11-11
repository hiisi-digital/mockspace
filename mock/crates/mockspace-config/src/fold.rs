//! A name as the letkajenkka standard spells it, for every mockspace crate that reads a reference.

/// `name` in the standard's character set (case lowered, a hyphen written as an underscore), or
/// `None` where no folding makes an identifier of it. The fold itself is the standard's.
#[must_use]
pub fn fold_name(name: &str) -> Option<String> {
    letkajenkka::ident::fold(name).map(|f| f.to_string())
}
