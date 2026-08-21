//! Which files a lint gets shown, decided by the runner rather than by each lint.
//!
//! Every lint that wanted to skip part of a tree ended up inventing its own answer.
//! `file_size` grew a `max_lines` param, `forbidden_imports` grew a `scope` string, and a
//! lint that wanted to skip generated code had no answer at all and read it anyway. Those
//! are three spellings of one question, and a fourth lint asking it would have produced a
//! fifth.
//!
//! So it gets answered once, here, and applied before a lint is called. A lint sees what it
//! was configured to see and cannot see the rest, which means no lint needs code for this
//! and one written without knowing the mechanism exists still respects it.
//!
//! ```toml
//! [lints.file-size]
//! exclude = ["**/generated/**", "*.pb.rs"]
//!
//! [lints.no-todo]
//! include = ["src/**"]
//! ```
//!
//! # What a pattern matches against
//!
//! The path the lint would report: crate-relative for a crate lint (`src/lib.rs`),
//! repo-relative for a repo lint. Separators are always `/`, on windows too, so one config
//! file describes one project everywhere.
//!
//! # The syntax, and what it does not have
//!
//! `?` matches one character inside a segment, `*` matches any run of them without crossing
//! a `/`, and `**` matches any number of whole segments including none.
//!
//! A pattern with no `/` in it matches the basename at any depth, so `exclude =
//! ["*.generated.rs"]` means what it looks like it means rather than matching only files
//! sitting at the root. That is what gitignore does and what anybody writing it will assume.
//!
//! Character classes (`[abc]`), brace expansion (`{a,b}`) and negated patterns are not
//! implemented. Each would want real syntax and nobody has asked for one yet; if that
//! changes, the honest move is probably to take the `glob` dependency rather than keep
//! growing this. Matching is naive backtracking, so a pathological pattern against a long
//! segment is exponential. That is fine for paths and would not be for arbitrary strings.
//!
//! # Include, then exclude
//!
//! With no `include`, every path is a candidate; with any, only the matching ones are.
//! `exclude` then removes from whatever survived, and wins wherever both match, on the
//! reasoning that the narrower statement is the one somebody wrote second.

use std::collections::HashMap;
use std::path::Path;

/// Which paths a single lint may be shown.
///
/// Empty means everything, which is what a lint with no path configuration gets, and is
/// why the runner can apply this unconditionally.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathFilter {
    /// Patterns a path has to match one of. Empty admits every path.
    pub include: Vec<String>,
    /// Patterns that remove a path. Beats `include`.
    pub exclude: Vec<String>,
}

impl PathFilter {
    /// Whether this filter would change anything. Worth asking before doing the work, since
    /// filtering a list down to itself is the common case.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// Whether a lint may be shown this path.
    #[must_use]
    pub fn allows(&self, path: &Path) -> bool {
        self.allows_str(&normalise(path))
    }

    /// [`Self::allows`], for a path already in `/`-separated form.
    #[must_use]
    pub fn allows_str(&self, path: &str) -> bool {
        if !self.include.is_empty() && !self.include.iter().any(|p| glob_match(p, path)) {
            return false;
        }
        !self.exclude.iter().any(|p| glob_match(p, path))
    }
}

/// Path filters per lint name, as parsed from `[lints.<name>]`.
pub type PathFilters = HashMap<String, PathFilter>;

/// A platform path as the patterns see it: `/` separators, no leading `./`.
fn normalise(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

/// Whether `pattern` matches `path`, both `/`-separated.
///
/// A pattern with no separator in it is lifted to `**/<pattern>` first, per the module docs.
#[must_use]
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let lifted;
    let pattern = if pattern.contains('/') {
        pattern
    } else {
        lifted = format!("**/{pattern}");
        &lifted
    };

    let pat: Vec<&str> = pattern.split('/').collect();
    let seg: Vec<&str> = path.split('/').collect();
    match_segments(&pat, &seg)
}

fn match_segments(pat: &[&str], path: &[&str]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        // `**` takes any number of whole segments including none, which is what lets the
        // lifted `**/foo.rs` reach a file at the root as well as a nested one.
        Some((&"**", rest)) => (0..=path.len()).any(|i| match_segments(rest, &path[i..])),
        Some((p, rest)) => match path.split_first() {
            Some((s, srest)) if match_one(p, s) => match_segments(rest, srest),
            _ => false,
        },
    }
}

/// `*` and `?` inside one segment. Neither crosses a separator, because the caller has
/// already split on it.
fn match_one(pat: &str, seg: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some(('*', rest)) => (0..=s.len()).any(|i| go(rest, &s[i..])),
            Some(('?', rest)) => !s.is_empty() && go(rest, &s[1..]),
            Some((c, rest)) => !s.is_empty() && s[0] == *c && go(rest, &s[1..]),
        }
    }
    let p: Vec<char> = pat.chars().collect();
    let s: Vec<char> = seg.chars().collect();
    go(&p, &s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_does_not_cross_a_separator() {
        assert!(glob_match("src/*.rs", "src/lib.rs"));
        assert!(!glob_match("src/*.rs", "src/inner/lib.rs"));
    }

    #[test]
    fn a_double_star_crosses_any_number_of_separators() {
        assert!(glob_match("src/**/mod.rs", "src/mod.rs"));
        assert!(glob_match("src/**/mod.rs", "src/a/mod.rs"));
        assert!(glob_match("src/**/mod.rs", "src/a/b/c/mod.rs"));
        assert!(!glob_match("src/**/mod.rs", "other/a/mod.rs"));
    }

    #[test]
    fn a_bare_pattern_reaches_any_depth() {
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(glob_match("*.rs", "src/lib.rs"));
        assert!(glob_match("*.rs", "src/a/b/lib.rs"));
        assert!(!glob_match("*.rs", "src/lib.md"));
    }

    #[test]
    fn a_pattern_with_a_separator_is_anchored() {
        // the other half of the case above, and why the lift is conditional: `src/lib.rs`
        // must not pick up a nested `a/src/lib.rs`
        assert!(glob_match("src/lib.rs", "src/lib.rs"));
        assert!(!glob_match("src/lib.rs", "a/src/lib.rs"));
    }

    #[test]
    fn question_mark_matches_exactly_one_character() {
        assert!(glob_match("v?.rs", "v1.rs"));
        assert!(!glob_match("v?.rs", "v12.rs"));
        assert!(!glob_match("v?.rs", "v.rs"));
    }

    #[test]
    fn an_empty_filter_admits_everything() {
        let f = PathFilter::default();
        assert!(f.is_empty());
        assert!(f.allows_str("anything/at/all.rs"));
    }

    #[test]
    fn include_alone_admits_only_what_matches() {
        let f = PathFilter { include: vec!["src/**".into()], exclude: vec![] };
        assert!(f.allows_str("src/lib.rs"));
        assert!(!f.allows_str("tests/it.rs"));
    }

    #[test]
    fn exclude_alone_removes_only_what_matches() {
        let f = PathFilter { include: vec![], exclude: vec!["**/generated/**".into()] };
        assert!(f.allows_str("src/lib.rs"));
        assert!(!f.allows_str("src/generated/pb.rs"));
    }

    #[test]
    fn exclude_beats_include_where_both_match() {
        let f = PathFilter {
            include: vec!["src/**".into()],
            exclude: vec!["**/generated/**".into()],
        };
        assert!(f.allows_str("src/lib.rs"));
        assert!(!f.allows_str("src/generated/pb.rs"));
    }

    #[test]
    fn a_windows_separator_is_matched_by_a_forward_slash_pattern() {
        // one config file describes one project on every platform, so the pattern language
        // has one separator and the path gets converted to it
        let f = PathFilter { include: vec!["src/**".into()], exclude: vec![] };
        assert!(f.allows(Path::new("src\\lib.rs")));
    }

    #[test]
    fn a_leading_dot_slash_is_not_a_segment() {
        assert!(glob_match("./src/lib.rs", "src/lib.rs"));
        assert!(
            PathFilter { include: vec!["src/**".into()], exclude: vec![] }
                .allows(Path::new("./src/lib.rs"))
        );
    }

    #[test]
    fn the_syntax_we_do_not_have_is_matched_literally_rather_than_ignored() {
        // this is the limit stated in the module docs, pinned so it fails loudly if
        // somebody adds character classes without updating what the docs promise
        assert!(glob_match("v[12].rs", "v[12].rs"));
        assert!(!glob_match("v[12].rs", "v1.rs"));
        assert!(!glob_match("a{b,c}.rs", "ab.rs"));
    }
}
