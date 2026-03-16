//! Lint: no `Vec<T>` in framework macro definitions.
//!
//! Framework types must use `Collection<T>` (from the storage crate)
//! instead of `Vec<T>`. `Collection<T>` integrates with the framework's
//! columnar storage, persistence, and shape introspection.
//!
//! Also catches `HashMap<`, `HashSet<`, `BTreeMap<`, `BTreeSet<` — all stdlib
//! collections should go through framework types.
//!
//! Scans inside all `define_*!` macro invocations.

use crate::{Lint, LintContext, LintError};

const FORBIDDEN: &[(&str, &str)] = &[
    // std::vec
    ("Vec<", "Collection<T>"),
    // std::collections
    ("HashMap<", "Collection<T> or Dictionary<K, V>"),
    ("HashSet<", "Collection<T>"),
    ("BTreeMap<", "Collection<T> or Dictionary<K, V>"),
    ("BTreeSet<", "Collection<T>"),
    ("VecDeque<", "Collection<T>"),
    ("LinkedList<", "Collection<T>"),
    ("BinaryHeap<", "Collection<T>"),
    // std::collections full paths
    ("std::collections::HashMap<", "Collection<T> or Dictionary<K, V>"),
    ("std::collections::HashSet<", "Collection<T>"),
    ("std::collections::BTreeMap<", "Collection<T> or Dictionary<K, V>"),
    ("std::collections::BTreeSet<", "Collection<T>"),
    ("std::collections::VecDeque<", "Collection<T>"),
    ("std::collections::LinkedList<", "Collection<T>"),
    ("std::collections::BinaryHeap<", "Collection<T>"),
    // indexmap (common dep)
    ("IndexMap<", "Collection<T> or Dictionary<K, V>"),
    ("IndexSet<", "Collection<T>"),
    // smallvec / tinyvec
    ("SmallVec<", "Collection<T>"),
    ("TinyVec<", "Collection<T>"),
    ("ArrayVec<", "Collection<T>"),
    // slotmap
    ("SlotMap<", "Collection<T>"),
    ("DenseSlotMap<", "Collection<T>"),
    ("SecondaryMap<", "Collection<T>"),
    // im (persistent collections)
    ("im::Vector<", "Collection<T>"),
    ("im::HashMap<", "Collection<T>"),
    ("im::HashSet<", "Collection<T>"),
    ("im::OrdMap<", "Collection<T>"),
    ("im::OrdSet<", "Collection<T>"),
];

const TRACKED_MACROS: &[&str] = &[
    "define_resource!",
    "define_record!",
    "define_signal!",
    "define_behavior!",
    "define_marker!",
    "define_binding!",
    "define_storage!",
];

pub struct NoVecInResource;

impl Lint for NoVecInResource {
    fn name(&self) -> &'static str {
        "no-vec-in-macros"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.is_proc_macro_crate() {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let mut in_macro = false;
        let mut brace_depth: i32 = 0;
        let mut macro_name = "";

        for (line_num, line) in ctx.source.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments
            if trimmed.starts_with("//") {
                continue;
            }

            // Detect macro entry
            if !in_macro {
                for &m in TRACKED_MACROS {
                    let prefix = &m[..m.len() - 1]; // strip '!'
                    if trimmed.contains(m) || trimmed.contains(prefix) {
                        in_macro = true;
                        brace_depth = 0;
                        macro_name = m;
                        break;
                    }
                }
            }

            if in_macro {
                brace_depth += line.matches('{').count() as i32;
                brace_depth -= line.matches('}').count() as i32;

                // Check field type annotations for forbidden collection types
                for &(pattern, replacement) in FORBIDDEN {
                    if trimmed.contains(pattern) {
                        errors.push(LintError {
                            crate_name: ctx.crate_name.to_string(),
                            line: line_num + 1,
                            lint_name: "no-vec-in-macros",
                        severity: crate::Severity::HARD_ERROR,
                            message: format!(
                                "`{pattern}..>` in {macro_name} — use `{replacement}` instead",
                            ),
                            finding_kind: None,
                        });
                    }
                }

                // Detect macro end
                if brace_depth <= 0 && (trimmed.ends_with(");") || trimmed == ")") {
                    in_macro = false;
                }
            }
        }

        errors
    }
}
