//! Panel seats: minted from one inventory file, capped at 99, with
//! enforced consolidation.
//!
//! # What a panel is, for this module's purposes
//!
//! A panel is several personas working one question, arguing, converging,
//! and proposing. Nothing in this module runs a panel or dispatches an
//! expert; that is an agent's job, done by whatever is reading the generated
//! `panel-discipline` rule this ships alongside (see
//! [`crate::render_agent::templates`]). What this module owns is the
//! **ledger**: how many seats a panel has minted, whether it has
//! consolidated recently enough to keep minting, and the one file that
//! records both, so neither fact is a claim anybody has to take on trust.
//!
//! # Why a seat is minted rather than counted
//!
//! An agent guessing the next seat number by counting how many it believes
//! have been used is exactly the failure this exists to prevent: two
//! dispatches counting concurrently mint the same number, and a seat number
//! that is merely believed rather than recorded is not a ledger at all. So
//! [`mint_seat`] is the only way a seat number is produced. It is derived
//! from the inventory that is about to be written back, not supplied by the
//! caller, which is what "minted against" means: the number comes from the
//! record, never the other way round.
//!
//! # Why 99 and not a config knob
//!
//! [`SEAT_CAP`] is a constant, not a project setting, because a panel that
//! has minted ninety-nine seats without converging has stopped being a
//! panel and started being a task queue. Ninety-nine is the design's own
//! number for "this has clearly gone on long enough"; letting a project
//! raise it would let the failure mode configure its own escape hatch.
//!
//! # Why consolidation cadence is per-inventory rather than per-project
//!
//! `mockspace.toml`'s `panel_consolidate_every` sets the default every new
//! inventory starts with ([`DEFAULT_CONSOLIDATE_EVERY`] when the project
//! sets nothing), but the inventory itself carries the value that actually
//! governs it (see [`PanelInventory::consolidate_every`]). A panel that
//! genuinely needs a different cadence (a wide cold-open phase minting many
//! seats before the first real convergence point, say) records that once,
//! in the file that already records everything else about it, rather than
//! needing a second project-wide setting to change for one panel's sake.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The last seat a panel may ever mint. Not configurable; see the module
/// doc for why.
pub const SEAT_CAP: u32 = 99;

/// How many seats mint before a consolidation is required, when neither the
/// inventory nor `mockspace.toml` says otherwise.
pub const DEFAULT_CONSOLIDATE_EVERY: u32 = 10;

/// One panel's whole ledger: every seat it has ever minted, in order, and
/// every consolidation recorded against it.
///
/// Read and written whole. A panel's lifetime is short enough (a handful to
/// a few dozen seats) that there is no case yet for anything more granular
/// than "load, mutate, save", and inventing one before a real project needs
/// it would be exactly the kind of unearned generality this project's own
/// rules argue against.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PanelInventory {
    /// The panel's own name, carried inside the file so it reads standalone
    /// even if it is ever moved or renamed on disk.
    pub slug: String,
    /// Every seat minted, oldest first.
    #[serde(rename = "seat")]
    pub seats: Vec<Seat>,
    /// Every consolidation recorded, oldest first.
    #[serde(rename = "consolidation")]
    pub consolidations: Vec<Consolidation>,
    /// This panel's own cadence override. `None` means fall back to
    /// whatever [`mint_seat`]'s caller was configured with (ultimately
    /// `mockspace.toml`'s `panel_consolidate_every`, or
    /// [`DEFAULT_CONSOLIDATE_EVERY`] if the project set nothing).
    pub consolidate_every: Option<u32>,
}

/// One minted seat.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Seat {
    /// 1-based, dense, and never reused: minting never fills a gap left by
    /// a seat that was never recorded, because there is no such gap. The
    /// number is always one past the highest number already in the file.
    pub number:        u32,
    /// Who holds the seat. A persona name, an agent identifier, whatever
    /// the caller names; this module has no opinion on the vocabulary.
    pub persona:       String,
    /// What the seat is working. Free text, same reasoning.
    pub topic:         String,
    /// Unix seconds at mint time. Not used to order anything (`seats` is
    /// already in mint order); kept because an audit trail with no
    /// timestamp at all invites the question of when, and the answer
    /// should not have to be reconstructed from git history.
    pub minted_at_unix: u64,
}

/// One recorded consolidation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Consolidation {
    /// The highest seat number this consolidation covers. Every seat
    /// numbered at or below this one counts as consolidated; every seat
    /// above it does not, regardless of when either was minted.
    pub after_seat: u32,
    /// What was decided. Free text; the discipline this module enforces is
    /// that a consolidation happened at all, not what it says.
    pub note:       String,
    /// Unix seconds at consolidation time.
    pub at_unix:    u64,
}

/// Why [`mint_seat`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MintRefusal {
    /// The panel has minted every seat it is ever allowed to. Consolidating
    /// does not help; the panel is done, and the next step is closing it
    /// (archiving the file) or opening a new one for whatever remains.
    SeatCapReached,
    /// `minted` seats have been minted since the last consolidation (or
    /// since the panel opened, if it has never consolidated), which meets
    /// or exceeds `cadence`. Consolidate before minting again.
    ConsolidationDue { minted: u32, cadence: u32 },
}

impl std::fmt::Display for MintRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SeatCapReached => {
                write!(f, "seat cap ({SEAT_CAP}) reached; this panel is done minting")
            },
            Self::ConsolidationDue {
                minted,
                cadence,
            } => {
                write!(
                    f,
                    "{minted} seat(s) minted since the last consolidation (cadence {cadence}); \
                     consolidate before minting another"
                )
            },
        }
    }
}

/// The seat number [`mint_seat`] would assign next, or `None` if the panel
/// has reached [`SEAT_CAP`].
///
/// Always one past the highest minted so far; an empty inventory's first
/// seat is 1. Exposed on its own (rather than only inside `mint_seat`)
/// because `mock panel status` wants to say what is coming next without
/// minting it.
#[must_use]
pub fn next_seat_number(inv: &PanelInventory) -> Option<u32> {
    let last = inv.seats.iter().map(|s| s.number).max().unwrap_or(0);
    if last >= SEAT_CAP {
        None
    } else {
        Some(last + 1)
    }
}

/// The highest seat number any consolidation covers, or 0 if none has ever
/// been recorded.
#[must_use]
pub fn last_consolidated_seat(inv: &PanelInventory) -> u32 {
    inv.consolidations.iter().map(|c| c.after_seat).max().unwrap_or(0)
}

/// How many seats have minted since the last consolidation (or since the
/// panel opened, if it has never consolidated).
#[must_use]
pub fn seats_since_last_consolidation(inv: &PanelInventory) -> u32 {
    let last = last_consolidated_seat(inv);
    u32::try_from(inv.seats.iter().filter(|s| s.number > last).count()).unwrap_or(u32::MAX)
}

/// Whether this panel is presently open: it has minted at least one seat
/// that no consolidation covers yet.
///
/// This is the signal `crate::entry::check`'s panel-discipline row reads:
/// an open panel is one whose seats have not been folded into a
/// consolidation, and per the panel-discipline rule this ships, canon is
/// not written while that is true.
#[must_use]
pub fn is_open(inv: &PanelInventory) -> bool {
    seats_since_last_consolidation(inv) > 0
}

/// The cadence this panel actually runs at: its own override if it has one,
/// else `project_default`.
#[must_use]
pub fn effective_cadence(inv: &PanelInventory, project_default: u32) -> u32 {
    inv.consolidate_every.unwrap_or(project_default)
}

/// Mint the next seat, mutating `inv` in place.
///
/// `now_unix` is a parameter rather than read from the clock in here, so
/// every caller (the CLI, and every test in this module) supplies it
/// explicitly and the result is reproducible.
///
/// Refuses per [`MintRefusal`] rather than silently doing nothing: a
/// refusal a caller cannot distinguish from success is a refusal that gets
/// worked around by whoever hits it, which is the exact discipline the seat
/// cap and the consolidation cadence both exist to prevent.
pub fn mint_seat(
    inv: &mut PanelInventory,
    persona: &str,
    topic: &str,
    project_default_cadence: u32,
    now_unix: u64,
) -> Result<u32, MintRefusal> {
    // The cap is checked first and is the only one of the two that cannot be
    // worked around: reaching it means the panel is over, and no
    // consolidation changes that. Checking it first says so plainly rather
    // than telling a capped panel to consolidate, which would look
    // actionable and would not be.
    let Some(number) = next_seat_number(inv) else {
        return Err(MintRefusal::SeatCapReached);
    };

    let cadence = effective_cadence(inv, project_default_cadence);
    if cadence > 0 {
        let minted = seats_since_last_consolidation(inv);
        if minted >= cadence {
            return Err(MintRefusal::ConsolidationDue {
                minted,
                cadence,
            });
        }
    }

    inv.seats.push(Seat {
        number,
        persona: persona.to_string(),
        topic: topic.to_string(),
        minted_at_unix: now_unix,
    });
    Ok(number)
}

/// Record a consolidation covering every seat minted so far, mutating `inv`
/// in place.
///
/// Returns the seat number it now covers, or `None` when there is nothing
/// new to consolidate (no seats at all, or every seat already covered by an
/// earlier consolidation). Recording a consolidation that covers nothing
/// would let a caller inflate the audit trail with entries that attest to
/// no actual convergence, which defeats the point of keeping one.
pub fn consolidate(inv: &mut PanelInventory, note: &str, now_unix: u64) -> Option<u32> {
    let last_seat = inv.seats.iter().map(|s| s.number).max()?;
    if last_seat <= last_consolidated_seat(inv) {
        return None;
    }
    inv.consolidations.push(Consolidation {
        after_seat: last_seat,
        note: note.to_string(),
        at_unix: now_unix,
    });
    Some(last_seat)
}

/// Where a panel's inventory lives: `<mock_dir>/panel/<slug>.toml`.
#[must_use]
pub fn inventory_path(mock_dir: &Path, slug: &str) -> PathBuf {
    mock_dir.join("panel").join(format!("{slug}.toml"))
}

/// Load a panel's inventory, or an empty one (with `slug` set) if the file
/// does not exist yet. A panel's first seat is minted against a file that
/// is not there yet, so "absent" has to mean "empty", not "an error".
pub fn load(path: &Path, slug: &str) -> Result<PanelInventory, String> {
    if !path.exists() {
        return Ok(PanelInventory {
            slug: slug.to_string(),
            ..PanelInventory::default()
        });
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut inv: PanelInventory = toml_edit::de::from_str(&text)
        .map_err(|e| format!("parsing {}: {e}", path.display()))?;
    if inv.slug.is_empty() {
        inv.slug = slug.to_string();
    }
    Ok(inv)
}

/// Write a panel's inventory back, creating `<mock_dir>/panel/` if it is
/// not there yet.
pub fn save(path: &Path, inv: &PanelInventory) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let text = toml_edit::ser::to_string_pretty(inv)
        .map_err(|e| format!("serialising {}: {e}", path.display()))?;
    std::fs::write(path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Every panel inventory declared under `<mock_dir>/panel/`, loaded. Used by
/// `crate::entry::check`'s panel-discipline row, which has to look at every
/// panel rather than one named on the command line.
#[must_use]
pub fn load_all(mock_dir: &Path) -> Vec<PanelInventory> {
    let dir = mock_dir.join("panel");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let slug = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            load(&p, &slug).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests;
