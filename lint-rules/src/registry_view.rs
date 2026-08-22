//! A read-only view of the registry, for a lint or a tool that checks it.
//!
//! The registry itself lives in the engine, which depends on this crate, so a
//! lint cannot name its types without inverting that. This is the flattened
//! shape instead: rows as fields, and the reverse edges precomputed, which is
//! everything a check actually asks and none of the loading, resolving or
//! rendering it does not.
//!
//! **The reverse edges are the reason this exists.** A row states what it
//! references; what references it is derived, and deriving it needs the field
//! types, which are configuration a lint has no other route to. Handing over the
//! computed answer keeps that knowledge in one place.
//!
//! Empty is a legitimate state and means a project with no registry at all. Every
//! accessor answers on an empty view without a special case, so a check written
//! against this cannot fail the way a lint that reached `crates.first()` used to.

use std::collections::BTreeMap;

/// One row's fields, by name, as the strings the loader stored.
pub type RowFields = BTreeMap<String, String>;

/// Every row the project declares, plus which rows reference which.
#[derive(Debug, Default, Clone)]
pub struct RegistryView {
    rows:         BTreeMap<String, RowFields>,
    by_namespace: BTreeMap<String, Vec<String>>,
    referrers:    BTreeMap<String, Vec<String>>,
}

impl RegistryView {
    /// Build from rows and reverse edges the engine has already computed.
    ///
    /// `rows` is keyed `namespace::slug`, and `referrers` maps a row to the rows
    /// naming it through a typed field. A caller passing an empty map for the
    /// second gets a view whose `referrers` answers empty for everything, which
    /// is indistinguishable from a project whose rows reference nothing. That is
    /// the caller's problem to avoid and is why the engine builds both together.
    pub fn new(
        rows: BTreeMap<String, RowFields>,
        referrers: BTreeMap<String, Vec<String>>,
    ) -> Self {
        let mut by_namespace: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for q in rows.keys() {
            if let Some((ns, _)) = q.split_once("::") {
                by_namespace.entry(ns.to_string()).or_default().push(q.clone());
            }
        }
        Self {
            rows,
            by_namespace,
            referrers,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Every namespace that has at least one row, in name order.
    ///
    /// A declared namespace with no rows does not appear, because this view is
    /// built from rows. A check about a namespace that declares nothing is a
    /// check about configuration rather than about data.
    pub fn namespaces(&self) -> impl Iterator<Item = &str> {
        self.by_namespace.keys().map(String::as_str)
    }

    /// The qualified identifiers of every row in one namespace, in slug order.
    pub fn rows_in(&self, namespace: &str) -> &[String] {
        self.by_namespace
            .get(namespace)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// One field of one row. `None` where the row or the field is absent, and
    /// those are deliberately not distinguished: a check asking for a field
    /// wants the value or nothing.
    pub fn field(&self, qualified: &str, name: &str) -> Option<&str> {
        self.rows.get(qualified)?.get(name).map(String::as_str)
    }

    pub fn row(&self, qualified: &str) -> Option<&RowFields> {
        self.rows.get(qualified)
    }

    /// Every row that references this one through a typed field, in name order.
    ///
    /// Empty means nothing references it. For a namespace enumerating what a
    /// project must answer, that emptiness is the finding.
    pub fn referrers(&self, qualified: &str) -> &[String] {
        self.referrers
            .get(qualified)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> RegistryView {
        let mut rows = BTreeMap::new();
        rows.insert(
            "slot::display".to_string(),
            [("use".to_string(), "pixels".to_string())].into_iter().collect(),
        );
        rows.insert("slot::audio".to_string(), RowFields::new());
        rows.insert(
            "answer::niri".to_string(),
            [("slot".to_string(), "display".to_string())].into_iter().collect(),
        );
        let referrers = [("slot::display".to_string(), vec!["answer::niri".to_string()])]
            .into_iter()
            .collect();
        RegistryView::new(rows, referrers)
    }

    #[test]
    fn rows_are_grouped_by_the_namespace_in_their_identifier() {
        let v = view();
        assert_eq!(v.rows_in("slot"), ["slot::audio", "slot::display"]);
        assert_eq!(v.rows_in("answer"), ["answer::niri"]);
        assert_eq!(v.namespaces().collect::<Vec<_>>(), ["answer", "slot"]);
    }

    /// The control: a namespace nothing declares answers empty rather than
    /// panicking, which is what lets a check run on a project with no registry.
    #[test]
    fn a_namespace_with_no_rows_answers_empty() {
        assert!(view().rows_in("nosuch").is_empty());
        assert!(RegistryView::default().rows_in("slot").is_empty());
        assert!(RegistryView::default().is_empty());
    }

    #[test]
    fn a_row_nothing_references_has_no_referrers() {
        let v = view();
        assert_eq!(v.referrers("slot::display"), ["answer::niri"]);
        assert!(
            v.referrers("slot::audio").is_empty(),
            "an unanswered row is the finding, and it is an empty list"
        );
    }

    /// The control on that: a row that does not exist also answers empty, so a
    /// check distinguishing the two must ask `row()` first rather than reading
    /// emptiness as absence.
    #[test]
    fn a_row_that_does_not_exist_also_has_no_referrers() {
        let v = view();
        assert!(v.referrers("slot::nosuch").is_empty());
        assert!(v.row("slot::nosuch").is_none());
        assert!(v.row("slot::audio").is_some());
    }

    #[test]
    fn a_field_reads_back_and_an_absent_one_is_none() {
        let v = view();
        assert_eq!(v.field("slot::display", "use"), Some("pixels"));
        assert_eq!(v.field("slot::display", "nosuch"), None);
        assert_eq!(v.field("slot::nosuch", "use"), None);
    }
}
