//! Store paths: relative, `/`-separated, no empty or dot components.

use crate::error::StoreError;

/// Check a path both backends can hold: it names a file, never leaves the
/// store, and has no character a git index line or a filename cannot carry.
pub fn validate_path(path: &str) -> Result<(), StoreError> {
    let bad = |why| {
        Err(StoreError::InvalidPath {
            path: path.to_owned(),
            why,
        })
    };
    if path.is_empty() {
        return bad("empty");
    }
    if path.starts_with('/') {
        return bad("absolute");
    }
    if path.chars().any(|c| c.is_control() || c == '\\') {
        return bad("control character or backslash");
    }
    for part in path.split('/') {
        match part {
            "" => return bad("empty component"),
            "." | ".." => return bad("dot component"),
            _ if part.starts_with(crate::dir::TMP_PREFIX) => {
                return bad("a name the directory backend keeps for files in flight");
            },
            _ if is_git_name(part) => return bad("a name git treats as its own"),
            _ => {},
        }
    }
    Ok(())
}

/// `.git` as git itself reads it: in any case, with trailing dots or spaces (as NTFS drops
/// them), and as the short name `git~1`. Git skips such a path silently when asked to store it.
fn is_git_name(part: &str) -> bool {
    let trimmed = part.trim_end_matches(['.', ' ']).to_ascii_lowercase();
    trimmed == ".git" || trimmed == "git~1"
}

/// Whether `path` is the entry `prefix` or lies under it. The empty prefix
/// matches everything.
pub fn under(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    prefix.is_empty()
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|r| r.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_pass() {
        for p in ["a", "a/b.toml", "x/y/z-1_2.json"] {
            assert!(validate_path(p).is_ok(), "{p}");
        }
    }

    #[test]
    fn paths_that_leave_or_confuse_are_refused() {
        for p in [
            "",
            "/a",
            "a//b",
            "a/",
            "../a",
            "a/../b",
            "./a",
            "a\\b",
            "a\nb",
            ".git/config",
            "a/.git/x",
        ] {
            assert!(validate_path(p).is_err(), "{p:?} should be refused");
        }
    }

    #[test]
    fn under_matches_whole_components_only() {
        assert!(under("a/b", "a"));
        assert!(under("a", "a"));
        assert!(under("a/b", ""));
        assert!(!under("ab/c", "a"));
        assert!(under("a/b/c", "a/b/"));
    }
}
