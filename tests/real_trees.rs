//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The real-tree gate for the benchspace loader.
//!
//! The nested-detection regression got through because every fixture
//! was built by the same hands that built the loader, and the one
//! shape that exposes the defect (a self-contained bench directory
//! inside a consumer tree) existed only in the real workspace. So
//! this test loads every real bench tree, found by walking a
//! workspace root handed in through `MOCKSPACE_REAL_TREES`.
//!
//! Without the variable the test PANICS rather than returning, and
//! it carries `#[ignore]` so a default run reports it as ignored.
//! An earlier shape printed a skip notice and returned: cargo
//! captures stderr for passing tests, so the notice was invisible
//! and the summary read `1 passed` having verified nothing. A skip
//! that looks like a pass is how a gate stops being one, and this
//! gate had never once run.
//!
//! Run it with:
//!   MOCKSPACE_REAL_TREES=<path to a directory of real projects> \
//!     cargo test --test real_trees -- --ignored

use std::path::{Path, PathBuf};

use mockspace_bench_harness::tree;

/// Every root bench tree under `workspace`: a `bench.toml` with no
/// ancestor `bench.toml` (a nested one is a member of its root, and
/// is loaded through it).
fn find_roots(workspace: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![workspace.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if name == "target" || name == ".git" || name == "node_modules" {
                    continue;
                }
                stack.push(path);
            } else if name == "bench.toml" {
                files.push(path);
            }
        }
    }
    let dirs: Vec<PathBuf> = files
        .iter()
        .filter_map(|f| f.parent().map(Path::to_path_buf))
        .collect();
    let mut roots: Vec<PathBuf> = dirs
        .iter()
        .filter(|d| !dirs.iter().any(|other| *d != other && d.starts_with(other)))
        .cloned()
        .collect();
    roots.sort();
    roots
}

#[test]
#[ignore = "needs MOCKSPACE_REAL_TREES pointing at a workspace of consumer clones"]
fn every_real_bench_tree_loads_and_resolves_through_the_benchspace_path() {
    let workspace = std::env::var("MOCKSPACE_REAL_TREES").unwrap_or_else(|_| {
        panic!(
            "the real-tree gate needs MOCKSPACE_REAL_TREES pointing at a \
             workspace of consumer clones. It panics rather than returning \
             because a silent return reads as a pass and verifies nothing."
        )
    });
    let workspace = PathBuf::from(workspace);
    let roots = find_roots(&workspace);
    assert!(
        !roots.is_empty(),
        "no bench trees under {}: the gate ran against nothing",
        workspace.display()
    );

    let mut composed_member_cells = 0usize;
    let mut total_cells = 0usize;
    for root in &roots {
        let tree =
            tree::load(root).unwrap_or_else(|e| panic!("{} failed to load: {e}", root.display()));
        let manifest = &tree.manifest;
        assert!(
            !manifest.bench.is_empty(),
            "{} loaded with zero benches",
            root.display()
        );
        for name in manifest.bench_names() {
            let sizes = manifest.bench[&name].sizes.len();
            assert!(sizes > 0, "{}/{name} has no points", root.display());
            for idx in 0 .. sizes {
                let cell = manifest
                    .for_size(&name, idx, root)
                    .unwrap_or_else(|e| panic!("{}/{name}[{idx}]: {e}", root.display()));
                assert!(!cell.variant_paths.is_empty());
                total_cells += 1;
                if manifest.nested.contains_key(&name) {
                    composed_member_cells += 1;
                    // member cells carry the member/section split and
                    // member-relative resolution
                    assert!(cell.nested);
                    assert_ne!(cell.bench, cell.bench_name);
                    let member_dir = root.join(&cell.bench);
                    assert!(
                        member_dir.join("bench.toml").is_file(),
                        "member cell {name} does not trace to a member dir"
                    );
                } else {
                    // root sections keep flat semantics exactly
                    assert!(!cell.nested);
                    assert_eq!(cell.bench, cell.bench_name);
                    assert_eq!(cell.sweep, cell.bench_name);
                }
            }
        }
        eprintln!(
            "{}: {} benches, {} member keys",
            root.display(),
            manifest.bench.len(),
            manifest.nested.len()
        );
    }

    // The workspace's one real sections-form member: one project's
    // resource_storage, whose deliberately trimmed [timing] must
    // survive composition. Pinned by content rather than by path so
    // a clone layout change does not silently drop the check.
    let sections_form = roots
        .iter()
        .find(|r| tree::load(r).is_ok_and(|t| !t.flat_members.is_empty()));
    if let Some(root) = sections_form {
        let tree = tree::load(root).unwrap();
        let member = tree.flat_members[0].clone();
        let key = tree
            .manifest
            .bench_names()
            .into_iter()
            .find(|k| k.starts_with(&format!("{member}/")))
            .expect("the member contributes keys");
        let cell = tree.manifest.for_size(&key, 0, root).unwrap();
        assert!(cell.nested);
        assert_eq!(cell.bench, member);
        let member_manifest = mockspace_bench_harness::config::BenchManifest::load(
            &root.join(&member).join("bench.toml"),
        )
        .unwrap();
        assert_eq!(
            cell.passes, member_manifest.timing.passes,
            "the member's own [timing] governs its sections"
        );
        assert_ne!(
            cell.passes,
            mockspace_bench_harness::config::TimingSection::default().passes,
            "the member budget is genuinely non-default, or this proves nothing"
        );
    } else {
        panic!(
            "no sections-form member found in the workspace; the shape this gate \
             exists for is missing, so the gate must fail rather than shrink"
        );
    }

    eprintln!("total: {total_cells} cells, {composed_member_cells} from members");
}
