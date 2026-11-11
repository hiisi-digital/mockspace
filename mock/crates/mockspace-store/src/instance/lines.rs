//! The gitignore-style lines that name an instance's files, written once for every place that
//! needs them: the repository's exclude file, which hides the instance from the branch's status,
//! and the sparse-checkout file, which keeps git from writing over it.
//!
//! The second is the first with each line's negation toggled: a sparse-checkout pattern includes
//! what an exclude pattern leaves in, so the lines that say "this is the instance" in one say
//! "this is not of the checkout" in the other. Both read a line as a gitignore pattern, so the
//! directory the instance sits in is escaped the same way, and a pattern is followed by the line
//! that takes directories back out of it, since an excluded directory is never looked into.

use super::held::Held;

/// `rel`, the instance's directory from the top of the work tree (`/`-separated, no slash at
/// either end, empty for the top itself), as the anchored prefix of a pattern: each character a
/// pattern reads as syntax is escaped, and the prefix starts with `/`. Empty for the top.
pub fn lead(rel: &str) -> String {
    let mut lead = String::new();
    for c in rel.chars() {
        if "\\*?[]!# ".contains(c) {
            lead.push('\\');
        }
        lead.push(c);
    }
    if !lead.is_empty() {
        lead.insert(0, '/');
    }
    lead
}

/// The exclude lines of an instance in `rel`: the whole directory, or each pattern of `held`
/// under it, followed by the line that takes directories back out of it, so that what stays
/// excluded is the files, which is what the instance holds.
pub fn exclude_lines(rel: &str, held: Option<&Held>) -> Vec<String> {
    let lead = lead(rel);
    match held {
        None => vec![format!("{lead}/")],
        Some(held) => held.patterns().flat_map(|p| [format!("{lead}/{p}"), format!("!{lead}/{p}/")]).collect(),
    }
}

/// The sparse-checkout lines that leave an instance's files out of the checkout: each exclude
/// line of [`exclude_lines`] with its negation toggled.
pub fn sparse_lines(rel: &str, held: Option<&Held>) -> Vec<String> {
    exclude_lines(rel, held)
        .into_iter()
        .map(|l| match l.strip_prefix('!') {
            Some(rest) => rest.to_owned(),
            None => format!("!{l}"),
        })
        .collect()
}
