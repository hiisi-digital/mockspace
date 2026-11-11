//! A name as the letkajenkka standard spells it.
//!
//! The standard has one character set, lowercase letters, digits and underscore, opening on a
//! letter, and leaves it to the host to fold its own naming into it. This is that fold, for every
//! mockspace crate that reads a reference: case lowered and a hyphen written as an underscore.

/// `name` in the standard's character set, or `None` where no folding makes an identifier of it
/// (it opens on a digit, holds a dot, or holds a character outside the set).
// FIXME(b9b5b5a8ef74): the registry crate holds the same fold in `refpath::fold`, because the two cargo
// workspaces cannot share a path dependency; the standard offering a host fold removes both.
#[must_use]
pub fn fold_name(name: &str) -> Option<String> {
    let folded: String = name
        .chars()
        .map(|c| if c == '-' { '_' } else { c.to_ascii_lowercase() })
        .collect();
    let bytes = folded.as_bytes();
    let ok = bytes.first().is_some_and(|b| letkajenkka::ident::is_start(*b))
        && bytes.iter().all(|b| letkajenkka::ident::is_body(*b));
    ok.then_some(folded)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mutation: dropping the case fold, or the hyphen fold, fails the first two rows.
    #[test]
    fn a_name_is_folded_into_the_one_character_set() {
        assert_eq!(fold_name("No-Heap").as_deref(), Some("no_heap"));
        assert_eq!(fold_name("stack-lints").as_deref(), Some("stack_lints"));
        assert_eq!(fold_name("no_std").as_deref(), Some("no_std"));
        assert_eq!(fold_name("2d"), None);
        assert_eq!(fold_name("a.b"), None);
        assert_eq!(fold_name(""), None);
        assert_eq!(fold_name("ärt"), None);
    }
}
