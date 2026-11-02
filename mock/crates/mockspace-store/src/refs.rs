//! The pair of refs a namespace lives on, and the refspec between them.

use crate::error::StoreError;

/// Where a namespace is kept locally and where it is kept on the remote.
/// Fetch and push both go through this pair, so the form of a namespace is a
/// setting: the same on both sides (`refs/mock/registry/ticket`), or a private
/// local ref over a branch on the remote (`refs/heads/mock/ticket`), which is
/// the fallback where a host takes no ref outside `refs/heads`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefPair {
    local:  String,
    remote: String,
}

/// Which form a namespace takes on the remote. The branch form is the default: measured
/// from a cloud container on 2 November 2026 (not by this crate's probe; the ticket `the_cloud_git_proxy_refuses_pushes_to_refs_outside_refs` of kaski holds the record),
/// the proxy accepted a push of `refs/heads/mock/probe` and hung up on `refs/mock/probe` and
/// `refs/notes/mock-probe`. The ref form is for a host that takes it, a self-hosted one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemoteForm {
    /// `refs/heads/mock/<namespace>` on the remote.
    #[default]
    Branch,
    /// `refs/mock/<namespace>` on the remote.
    Ref,
}

impl RefPair {
    /// The pair for `namespace` in the given remote form.
    pub fn for_namespace(namespace: &str, form: RemoteForm) -> Result<Self, StoreError> {
        match form {
            RemoteForm::Branch => Self::branch_form(namespace),
            RemoteForm::Ref => Self::ref_form(namespace),
        }
    }

    /// An explicit pair. Both names must be fully qualified refs.
    pub fn new(local: impl Into<String>, remote: impl Into<String>) -> Result<Self, StoreError> {
        let (local, remote) = (local.into(), remote.into());
        validate_ref(&local)?;
        validate_ref(&remote)?;
        Ok(Self {
            local,
            remote,
        })
    }

    /// `refs/mock/<namespace>` on both sides.
    pub fn ref_form(namespace: &str) -> Result<Self, StoreError> {
        validate_namespace(namespace)?;
        let name = format!("refs/mock/{namespace}");
        Self::new(name.clone(), name)
    }

    /// `refs/mock/<namespace>` locally, the branch `refs/heads/mock/<namespace>` on the remote.
    pub fn branch_form(namespace: &str) -> Result<Self, StoreError> {
        validate_namespace(namespace)?;
        Self::new(
            format!("refs/mock/{namespace}"),
            format!("refs/heads/mock/{namespace}"),
        )
    }

    pub fn local(&self) -> &str {
        &self.local
    }

    pub fn remote(&self) -> &str {
        &self.remote
    }

    /// The forced fetch refspec, `+<remote>:<local>`. Forced because the local
    /// ref is a private mirror of the remote's, never a place to keep work.
    pub fn fetch_refspec(&self) -> String {
        format!("+{}:{}", self.remote, self.local)
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
    fn the_two_forms_differ_only_on_the_remote_side() {
        let r = RefPair::ref_form("registry/ticket").unwrap();
        assert_eq!(
            (r.local(), r.remote()),
            ("refs/mock/registry/ticket", "refs/mock/registry/ticket")
        );
        let b = RefPair::branch_form("registry/ticket").unwrap();
        assert_eq!(b.local(), r.local());
        assert_eq!(b.remote(), "refs/heads/mock/registry/ticket");
        assert_eq!(
            b.fetch_refspec(),
            "+refs/heads/mock/registry/ticket:refs/mock/registry/ticket"
        );
    }

    #[test]
    fn the_default_form_is_the_branch() {
        assert_eq!(RemoteForm::default(), RemoteForm::Branch);
        let p = RefPair::for_namespace("registry/ticket", RemoteForm::default()).unwrap();
        assert_eq!(p.remote(), "refs/heads/mock/registry/ticket");
        let r = RefPair::for_namespace("registry/ticket", RemoteForm::Ref).unwrap();
        assert_eq!(r.remote(), "refs/mock/registry/ticket");
    }

    #[test]
    fn bad_names_are_refused() {
        for ns in ["", "A", "a b", "a//b", ".hidden", "x.lock", "a:b"] {
            assert!(RefPair::ref_form(ns).is_err(), "{ns:?}");
        }
        for r in [
            "heads/x",
            "refs/a b",
            "refs/a..b",
            "refs/a:b",
            "refs/x/",
            "refs/x.lock",
            "refs/a*",
        ] {
            assert!(RefPair::new(r, "refs/mock/x").is_err(), "{r:?}");
        }
    }
}
