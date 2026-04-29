use std::collections::{BTreeMap, BTreeSet};

use crate::model::*;

/// Compute the transitive set of dependencies reachable from `name`.
///
/// Iterative work-stack walk with a visited set so dep cycles cannot
/// blow the call stack (#267). The result excludes `name` itself; it
/// contains every dep reachable through one or more edges.
pub fn all_transitive(
    name: &str,
    crates: &CrateMap,
    cache: &mut BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    if let Some(cached) = cache.get(name) {
        return cached.clone();
    }
    let mut result = BTreeSet::new();
    let mut stack: Vec<String> = Vec::new();
    if let Some(info) = crates.get(name) {
        for dep in &info.deps {
            stack.push(dep.clone());
        }
    }
    while let Some(curr) = stack.pop() {
        if !result.insert(curr.clone()) {
            continue;
        }
        if let Some(info) = crates.get(curr.as_str()) {
            for dep in &info.deps {
                if !result.contains(dep) {
                    stack.push(dep.clone());
                }
            }
        }
    }
    cache.insert(name.to_string(), result.clone());
    result
}

pub fn transitive_reduction(crates: &CrateMap) -> BTreeMap<String, Vec<String>> {
    let mut cache = BTreeMap::new();
    let mut reduced = BTreeMap::new();
    for (name, info) in crates {
        let mut keep = Vec::new();
        for dep in &info.deps {
            let reachable = info.deps.iter().any(|other| {
                other != dep && all_transitive(other, crates, &mut cache).contains(dep)
            });
            if !reachable {
                keep.push(dep.clone());
            }
        }
        reduced.insert(name.clone(), keep);
    }
    reduced
}

/// Compute the longest-path depth of `root` over its dep DAG.
///
/// Iterative post-order DFS with explicit Enter/Exit work items so dep
/// cycles cannot blow the call stack (#267). Cycle back-edges resolve
/// to depth 0 for the unresolved member, terminating the computation.
pub fn compute_depth(
    root: &str,
    crates: &CrateMap,
    cache: &mut BTreeMap<String, usize>,
) -> usize {
    enum Step {
        Enter(String),
        Exit(String),
    }
    let mut work: Vec<Step> = vec![Step::Enter(root.to_string())];
    let mut on_stack: BTreeSet<String> = BTreeSet::new();
    while let Some(step) = work.pop() {
        match step {
            Step::Enter(name) => {
                if cache.contains_key(&name) || on_stack.contains(&name) {
                    continue;
                }
                on_stack.insert(name.clone());
                work.push(Step::Exit(name.clone()));
                if let Some(info) = crates.get(name.as_str()) {
                    for dep in &info.deps {
                        if !cache.contains_key(dep) && !on_stack.contains(dep) {
                            work.push(Step::Enter(dep.clone()));
                        }
                    }
                }
            }
            Step::Exit(name) => {
                on_stack.remove(&name);
                let d = crates
                    .get(name.as_str())
                    .map(|info| {
                        info.deps
                            .iter()
                            .map(|dep| cache.get(dep).copied().map(|d| d + 1).unwrap_or(0))
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                cache.insert(name, d);
            }
        }
    }
    cache.get(root).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CrateInfo;

    fn make_crate(deps: &[&str]) -> CrateInfo {
        CrateInfo {
            short_name: String::new(),
            items: Vec::new(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            macro_generated: Vec::new(),
        }
    }

    fn map(entries: &[(&str, &[&str])]) -> CrateMap {
        let mut m = CrateMap::new();
        for (name, deps) in entries {
            m.insert(name.to_string(), make_crate(deps));
        }
        m
    }

    #[test]
    fn linear_chain_depth() {
        let crates = map(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
        let mut cache = BTreeMap::new();
        assert_eq!(compute_depth("a", &crates, &mut cache), 2);
        assert_eq!(compute_depth("b", &crates, &mut cache), 1);
        assert_eq!(compute_depth("c", &crates, &mut cache), 0);
    }

    #[test]
    fn diamond_depth() {
        let crates = map(&[
            ("a", &["b", "c"]),
            ("b", &["d"]),
            ("c", &["d"]),
            ("d", &[]),
        ]);
        let mut cache = BTreeMap::new();
        assert_eq!(compute_depth("a", &crates, &mut cache), 2);
        assert_eq!(compute_depth("d", &crates, &mut cache), 0);
    }

    #[test]
    fn linear_chain_transitive() {
        let crates = map(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
        let mut cache = BTreeMap::new();
        let t = all_transitive("a", &crates, &mut cache);
        assert!(t.contains("b") && t.contains("c"));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn cycle_does_not_overflow_transitive() {
        // a -> b -> a, plus a -> c. Old recursive code stack-overflows here.
        let crates = map(&[("a", &["b", "c"]), ("b", &["a"]), ("c", &[])]);
        let mut cache = BTreeMap::new();
        let t = all_transitive("a", &crates, &mut cache);
        // result is the set of dep nodes reached, including the back-edge target
        assert!(t.contains("b"));
        assert!(t.contains("c"));
    }

    #[test]
    fn cycle_does_not_overflow_depth() {
        // Self-cycle plus deeper structure.
        let crates = map(&[("a", &["b"]), ("b", &["a", "c"]), ("c", &[])]);
        let mut cache = BTreeMap::new();
        // Just assert termination; exact depth on a cycle is implementation-defined.
        let _ = compute_depth("a", &crates, &mut cache);
        let _ = compute_depth("b", &crates, &mut cache);
        let _ = compute_depth("c", &crates, &mut cache);
    }

    #[test]
    fn missing_node_yields_empty() {
        let crates = map(&[("a", &[])]);
        let mut t_cache = BTreeMap::new();
        assert!(all_transitive("missing", &crates, &mut t_cache).is_empty());
        let mut d_cache = BTreeMap::new();
        assert_eq!(compute_depth("missing", &crates, &mut d_cache), 0);
    }
}
