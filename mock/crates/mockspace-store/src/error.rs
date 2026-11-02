//! The one error type of the store. Every variant names what failed and carries
//! git's own words, so a caller can print it as it is.

use core::fmt;

/// Why a store call failed.
#[derive(Debug)]
pub enum StoreError {
    /// A path in an edit or a read is not a plain relative `/`-separated path.
    InvalidPath {
        path: String,
        why:  &'static str,
    },
    /// A ref name or a namespace is not one the store will use.
    InvalidRef {
        name: String,
        why:  &'static str,
    },
    /// The local ref is missing and no fetch has confirmed that the remote has
    /// none either. A read here would look like an empty store, so it refuses.
    NotFetched {
        local_ref: String,
    },
    /// A fetch failed: the remote is unreachable, or refused, or the ref could
    /// not be brought over. Never an empty store.
    Fetch {
        remote:  String,
        refspec: String,
        detail:  String,
    },
    /// The remote tip does not descend from the one this clone holds: history was rewritten
    /// there. The local ref is left as it was.
    RemoteRewrote {
        remote_ref: String,
        local_tip:  String,
        remote_tip: String,
    },
    /// The remote no longer has a ref this store holds locally. Silently
    /// dropping the local copy would lose data, so it is an error.
    RemoteLostRef {
        remote_ref: String,
        local_tip:  String,
    },
    /// The push was refused for a reason other than a lost race.
    PushRefused {
        remote:     String,
        remote_ref: String,
        detail:     String,
    },
    /// The push was accepted but the remote does not hold the commit afterwards.
    Verify {
        remote_ref: String,
        commit:     String,
        detail:     String,
    },
    /// Writers kept winning the race until the attempts ran out.
    RaceLost {
        attempts: u32,
    },
    /// The merge hook refused to rebase an edit on the tip that won.
    Merge {
        reason: String,
    },
    /// A git command failed in a way none of the variants above names.
    Git {
        command: String,
        detail:  String,
    },
    /// A filesystem call of the directory backend failed.
    Io {
        context: String,
        source:  std::io::Error,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath {
                path,
                why,
            } => write!(f, "invalid store path `{path}`: {why}"),
            Self::InvalidRef {
                name,
                why,
            } => write!(f, "invalid ref `{name}`: {why}"),
            Self::NotFetched {
                local_ref,
            } => {
                write!(
                    f,
                    "`{local_ref}` is absent and no fetch has confirmed the remote has none; refusing to read it as an empty store"
                )
            },
            Self::Fetch {
                remote,
                refspec,
                detail,
            } => {
                write!(f, "fetch of `{refspec}` from `{remote}` failed: {detail}")
            },
            Self::RemoteRewrote {
                remote_ref,
                local_tip,
                remote_tip,
            } => {
                write!(
                    f,
                    "remote `{remote_ref}` is at {remote_tip}, which does not descend from {local_tip} held here; history was rewritten there, and is not followed"
                )
            },
            Self::RemoteLostRef {
                remote_ref,
                local_tip,
            } => {
                write!(
                    f,
                    "remote no longer has `{remote_ref}` but this clone holds it at {local_tip}; not dropping it"
                )
            },
            Self::PushRefused {
                remote,
                remote_ref,
                detail,
            } => {
                write!(f, "push to `{remote}` `{remote_ref}` refused: {detail}")
            },
            Self::Verify {
                remote_ref,
                commit,
                detail,
            } => {
                write!(
                    f,
                    "pushed {commit} to `{remote_ref}` but could not confirm the remote holds it: {detail}"
                )
            },
            Self::RaceLost {
                attempts,
            } => {
                write!(f, "lost the push race on every one of {attempts} attempts")
            },
            Self::Merge {
                reason,
            } => write!(f, "merge hook refused: {reason}"),
            Self::Git {
                command,
                detail,
            } => write!(f, "`git {command}` failed: {detail}"),
            Self::Io {
                context,
                source,
            } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io {
                source,
                ..
            } => Some(source),
            _ => None,
        }
    }
}
