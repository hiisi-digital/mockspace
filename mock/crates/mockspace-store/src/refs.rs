//! The pair of refs a mock tree lives on, and the refspecs between them.

use crate::error::StoreError;

/// Where every mock tree is kept on the remote: orphan custom refs under this prefix, never under
/// `refs/heads/`. Fetchable by exact name, listed by mockspace's tools only, and not private. Op,
/// 23 November 2026: "mocksoace refs are NOT in heads. Entirely orphan ... not discoverable
/// without mockspace tooling is what we go for; not entirely private".
pub const MOCK_REFS: &str = "refs/mock/";

/// The namespace of the shadow line, the one mock tree that branches with the code. A data tree's
/// namespace may not be it: [`validate_namespace`] takes only lowercase letters, digits and
/// `- _ .`, and `+` is none of them.
pub const SHADOW_NAMESPACE: &str = "+";

/// The prefix the shadow branches take locally, `refs/heads/+/<name>`. A custom ref cannot be the
/// current branch (`git branch --show-current` answers "HEAD not found below refs/heads!" and a
/// checkout of one detaches), so locally a shadow branch is an ordinary branch, and the prefix
/// keeps `dev` from meaning two things in one clone.
pub const SHADOW_BRANCHES: &str = "refs/heads/+/";

/// Where the instance keeps the last tip it saw of each mock tree, `refs/mock-seen/<namespace>`.
/// Only the store moves it, so a forced fetch by plain git that moves the local ref onto a
/// rewritten remote does not move this, and the next fetch through the store refuses that tip.
pub const SEEN_REFS: &str = "refs/mock-seen/";

/// Where a mock tree is kept locally and where it is kept on the remote. Fetch and push both go
/// through this pair. A data tree is `refs/mock/<namespace>` on both sides, a private local
/// mirror of the remote's ref; a shadow branch is the ordinary local branch
/// `refs/heads/+/<name>` over `refs/mock/+/<name>` on the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefPair {
    local:  String,
    remote: String,
    seen:   String,
}

impl RefPair {
    /// The pair of the data tree `namespace`: `refs/mock/<namespace>` locally and on the remote.
    pub fn for_namespace(namespace: &str) -> Result<Self, StoreError> {
        validate_namespace(namespace)?;
        let name = format!("{MOCK_REFS}{namespace}");
        Self::new(name.clone(), name)
    }

    /// The pair of the shadow branch `name` (`dev`, `work/<topic>`): the local branch
    /// `refs/heads/+/<name>` over `refs/mock/+/<name>` on the remote.
    pub fn shadow(name: &str) -> Result<Self, StoreError> {
        validate_ref(&format!("refs/heads/{name}"))?;
        Self::new(
            format!("{SHADOW_BRANCHES}{name}"),
            format!("{MOCK_REFS}{SHADOW_NAMESPACE}/{name}"),
        )
    }

    /// An explicit pair. Both names must be fully qualified refs, and the remote one a mock ref.
    pub fn new(local: impl Into<String>, remote: impl Into<String>) -> Result<Self, StoreError> {
        let (local, remote) = (local.into(), remote.into());
        validate_ref(&local)?;
        validate_ref(&remote)?;
        let Some(tree) = remote.strip_prefix(MOCK_REFS) else {
            return Err(StoreError::InvalidRef {
                name: remote,
                why:  "a mock tree is kept on the remote under refs/mock/, never under refs/heads/ or elsewhere",
            });
        };
        let seen = format!("{SEEN_REFS}{tree}");
        Ok(Self {
            local,
            remote,
            seen,
        })
    }

    pub fn local(&self) -> &str {
        &self.local
    }

    pub fn remote(&self) -> &str {
        &self.remote
    }

    /// The backup ref of the last tip this clone saw of the tree, `refs/mock-seen/<tree>`.
    pub fn seen(&self) -> &str {
        &self.seen
    }

    /// The forced fetch refspec, `+<remote>:<local>`. Forced because the local ref is a private
    /// mirror of the remote's, never a place to keep work; what keeps a rewrite out is the check
    /// against [`RefPair::seen`], not git.
    pub fn fetch_refspec(&self) -> String {
        format!("+{}:{}", self.remote, self.local)
    }

    /// The push refspecs of the shadow line in a clone's git config, so a plain `git push` of a
    /// shadow branch lands on its custom ref: `refs/heads/+/*:refs/mock/+/*`. Not forced: a
    /// shadow branch moves forward only.
    pub fn shadow_push_refspec() -> String {
        format!("{SHADOW_BRANCHES}*:{MOCK_REFS}{SHADOW_NAMESPACE}/*")
    }

    /// The fetch refspec of the shadow line, into remote-tracking refs and never over a local
    /// branch, which may hold work: `+refs/mock/+/*:refs/remotes/<remote>/+/*`. Written fully
    /// qualified, since a leading `+` is git's force and the namespace is `+` too.
    pub fn shadow_fetch_refspec(remote: &str) -> String {
        format!("+{MOCK_REFS}{SHADOW_NAMESPACE}/*:refs/remotes/{remote}/{SHADOW_NAMESPACE}/*")
    }
}

fn validate_namespace(ns: &str) -> Result<(), StoreError> {
    let bad = |why| {
        Err(StoreError::InvalidRef {
            name: ns.to_owned(),
            why,
        })
    };
    if ns.is_empty() {
        return bad("empty namespace");
    }
    for part in ns.split('/') {
        if part.is_empty() || part.starts_with('.') || part.ends_with(".lock") {
            return bad("empty, dotted or `.lock` component");
        }
        if !part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
        {
            return bad("only lowercase letters, digits and `- _ .` in a component");
        }
    }
    Ok(())
}

fn validate_ref(name: &str) -> Result<(), StoreError> {
    let bad = |why| {
        Err(StoreError::InvalidRef {
            name: name.to_owned(),
            why,
        })
    };
    if !name.starts_with("refs/") {
        return bad("not a fully qualified ref");
    }
    if name.ends_with('/')
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
        || name.contains("@{")
    {
        return bad("malformed");
    }
    if name.chars().any(|c| {
        c.is_control() || c.is_whitespace() || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    }) {
        return bad("a character git refuses in a ref name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_data_tree_is_the_same_custom_ref_on_both_sides() {
        let r = RefPair::for_namespace("registry/ticket").unwrap();
        assert_eq!((r.local(), r.remote()), ("refs/mock/registry/ticket", "refs/mock/registry/ticket"));
        assert_eq!(r.fetch_refspec(), "+refs/mock/registry/ticket:refs/mock/registry/ticket");
        assert_eq!(r.seen(), "refs/mock-seen/registry/ticket");
    }

    #[test]
    fn no_pair_puts_a_mock_tree_under_refs_heads_on_the_remote() {
        for remote in ["refs/heads/mock/registry", "refs/notes/mock", "refs/tags/mock"] {
            assert!(RefPair::new("refs/mock/registry", remote).is_err(), "{remote}");
        }
    }

    #[test]
    fn a_shadow_branch_is_an_ordinary_local_branch_over_a_custom_ref() {
        let s = RefPair::shadow("work/topic").unwrap();
        assert_eq!((s.local(), s.remote()), ("refs/heads/+/work/topic", "refs/mock/+/work/topic"));
        assert_eq!(s.seen(), "refs/mock-seen/+/work/topic");
        assert_eq!(RefPair::shadow_push_refspec(), "refs/heads/+/*:refs/mock/+/*");
        assert_eq!(RefPair::shadow_fetch_refspec("origin"), "+refs/mock/+/*:refs/remotes/origin/+/*");
    }

    #[test]
    fn no_data_tree_takes_the_shadow_namespace() {
        assert!(RefPair::for_namespace(SHADOW_NAMESPACE).is_err());
        assert!(RefPair::for_namespace("+/dev").is_err());
    }

    #[test]
    fn bad_names_are_refused() {
        for ns in ["", "A", "a b", "a//b", ".hidden", "x.lock", "a:b"] {
            assert!(RefPair::for_namespace(ns).is_err(), "{ns:?}");
        }
        for name in ["", "a b", "a..b", "x.lock", "a:b"] {
            assert!(RefPair::shadow(name).is_err(), "{name:?}");
        }
        for r in ["heads/x", "refs/a b", "refs/a..b", "refs/a:b", "refs/x/", "refs/x.lock", "refs/a*"] {
            assert!(RefPair::new(r, "refs/mock/x").is_err(), "{r:?}");
        }
    }
}
