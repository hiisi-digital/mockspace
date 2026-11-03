//! Who a commit on a data ref is by.

/// The author and committer of the commits the store writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name:  String,
    pub email: String,
}

impl Identity {
    /// v2's synthetic identity, `mockspace <noreply@mockspace.local>`
    /// (`mockspace-core` `io/ref_write.rs`), the same on every machine.
    pub fn synthetic() -> Self {
        Self {
            name:  "mockspace".into(),
            email: "noreply@mockspace.local".into(),
        }
    }

    /// A named identity pinned by the caller, as `msg` pins its author.
    pub fn pinned(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            name:  name.into(),
            email: email.into(),
        }
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self::synthetic()
    }
}
