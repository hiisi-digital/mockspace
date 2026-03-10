use std::collections::{BTreeMap, BTreeSet};

use crate::model::*;

pub fn all_transitive(
    name: &str,
    crates: &CrateMap,
    cache: &mut BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    if let Some(cached) = cache.get(name) {
        return cached.clone();
    }
    let mut result = BTreeSet::new();
    if let Some(info) = crates.get(name) {
        for dep in &info.deps {
            result.insert(dep.clone());
            result.extend(all_transitive(dep, crates, cache));
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

pub fn compute_depth(
    name: &str,
    crates: &CrateMap,
    cache: &mut BTreeMap<String, usize>,
) -> usize {
    if let Some(&d) = cache.get(name) {
        return d;
    }
    let d = crates
        .get(name)
        .map(|info| {
            info.deps
                .iter()
                .map(|dep| compute_depth(dep, crates, cache) + 1)
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    cache.insert(name.to_string(), d);
    d
}
