//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

// Each test names the mutant that turns it red.

fn touch(dir: &Path, name: &str, text: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(name), text).unwrap();
}

fn listing(dir: &Path) -> Vec<String> {
    names_in(dir)
}

fn closed_round(dr: &Path, dir: &str) {
    let d = dr.join(dir);
    touch(&d, "202610221529_changelist.doc.lock.md", "# doc\nbody\n");
    touch(&d, "202610221529_changelist.src.lock.md", "# src\nbody\n");
    touch(&d, "202610221525_topic.earlier.md", "x");
    touch(&d, ".meta", &format!("round: {dir}\nclosed: abc\nclose_date: 2026-10-22\nabandoned: false\n"));
    touch(&d, ".history", "abc one\n");
}

#[test]
fn a_closed_round_is_renamed_whole() {
    // Mutants: the directory is not moved; the files keep the old id; `.meta` keeps
    // the old `round:`; a file carrying another stamp is renamed too.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529");
    let r = rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap();
    assert_eq!(r.new_name, "202610221529-kx3f");
    assert!(!dr.join("202610221529").exists());
    let new = dr.join("202610221529-kx3f");
    assert_eq!(
        listing(&new),
        [
            ".history",
            ".meta",
            "202610221525_topic.earlier.md",
            "202610221529-kx3f_changelist.doc.lock.md",
            "202610221529-kx3f_changelist.src.lock.md",
        ]
    );
    let meta = fs::read_to_string(new.join(".meta")).unwrap();
    assert!(meta.starts_with("round: 202610221529-kx3f\n"), "{meta}");
    assert!(meta.contains("closed: abc\n") && meta.contains("abandoned: false\n"), "{meta}");
    assert_eq!(r.rewrote, [new.join(".meta")]);
    // every file that moved is listed, for the commit
    assert_eq!(r.moves.len(), 5);
    assert!(r.moves.iter().all(|(from, to)| !from.exists() && to.exists()));
}

#[test]
fn the_renamed_round_still_reads_as_a_round() {
    // Mutant: the new names are not parsed by the changelist reader.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529");
    rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap();
    let cls = changelist_helpers::find_changelists(&dr.join("202610221529-kx3f"));
    assert_eq!(cls.len(), 2);
    assert!(lint_rules::round_id::is_round_dir("202610221529-kx3f"));
}

#[test]
fn an_abandoned_round_keeps_its_suffix_and_a_count_goes() {
    // Mutant: `kept_tail` keeps `-2`, or drops `-abandoned`.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529-abandoned");
    let r = rename_round_on_disk(dr, "202610221529-abandoned", "202610221529-kx3f").unwrap();
    assert_eq!(r.new_name, "202610221529-kx3f-abandoned");
    closed_round(dr, "202610221530-2");
    let r = rename_round_on_disk(dr, "202610221530-2", "202610221530-aaaa").unwrap();
    assert_eq!(r.new_name, "202610221530-aaaa");
}

#[test]
fn the_active_round_is_renamed_by_its_flat_files() {
    // Mutant: flat files are not looked for, or a topic of another stamp moves.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    touch(dr, "202610221529_changelist.doc.md", "x");
    touch(dr, "202610221529_topic.a.md", "x");
    touch(dr, "202610221500_topic.other.md", "x");
    touch(dr, "README.md", "x");
    let r = rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap();
    assert_eq!(r.new_name, "202610221529-kx3f");
    assert_eq!(
        listing(dr),
        [
            "202610221500_topic.other.md",
            "202610221529-kx3f_changelist.doc.md",
            "202610221529-kx3f_topic.a.md",
            "README.md",
        ]
    );
}

#[test]
fn a_directory_wins_over_flat_files_with_the_same_id() {
    // Mutant: the flat branch runs first.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529");
    touch(dr, "202610221529_topic.next.md", "x");
    rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap();
    assert!(dr.join("202610221529_topic.next.md").exists());
    assert!(dr.join("202610221529-kx3f").is_dir());
}

#[test]
fn a_target_that_exists_is_refused_and_nothing_moves() {
    // Mutant: `rename` overwrites, or is attempted before the check.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529");
    closed_round(dr, "202610221529-kx3f");
    let err = rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    assert!(dr.join("202610221529/202610221529_changelist.doc.lock.md").exists());

    touch(dr, "202610221600_topic.a.md", "x");
    touch(dr, "202610221600-aaaa_topic.a.md", "x");
    let err = rename_round_on_disk(dr, "202610221600", "202610221600-aaaa").unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    assert!(dr.join("202610221600_topic.a.md").exists());
}

#[test]
fn an_unknown_round_or_a_bad_new_id_is_an_error() {
    // Mutant: a missing round is a silent success; a malformed id is taken.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    assert!(rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").is_err());
    closed_round(dr, "202610221529");
    assert!(rename_round_on_disk(dr, "202610221529", "202610221529-KX3F").is_err());
    assert!(rename_round_on_disk(dr, "202610221529", "2026").is_err());
    assert!(rename_round_on_disk(dr, "README", "202610221529-kx3f").is_err());
}

#[test]
fn only_the_round_line_of_meta_is_rewritten() {
    // Mutant: a blanket replace of the old id in the text.
    let text = "round: 202610221529\nclosed: 202610221529\nnote: round: 202610221529\r\n";
    assert_eq!(
        rewrite_meta(text, "202610221529", "202610221529-kx3f"),
        "round: 202610221529-kx3f\nclosed: 202610221529\nnote: round: 202610221529\r\n"
    );
    assert_eq!(
        rewrite_meta("round: 202610221529\r\nx\r\n", "202610221529", "202610221529-kx3f"),
        "round: 202610221529-kx3f\r\nx\r\n"
    );
}

#[test]
fn an_abandoned_round_with_a_count_stays_abandoned() {
    // Mutant: `kept_tail` knows `-abandoned` only whole, so `-abandoned-2` is
    // renamed to a bare tagged id, which `is_round_dir` reads as a closed round.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529-abandoned-2");
    let r = rename_round_on_disk(dr, "202610221529-abandoned-2", "202610221529-kx3f").unwrap();
    assert_eq!(r.new_name, "202610221529-kx3f-abandoned");
    assert!(!lint_rules::round_id::is_round_dir(&r.new_name));
    let meta = fs::read_to_string(dr.join(&r.new_name).join(".meta")).unwrap();
    assert!(
        meta.starts_with("round: 202610221529-kx3f-abandoned\n"),
        "{meta}"
    );
    closed_round(dr, "202610221530-kx3f-abandoned-3");
    let r = rename_round_on_disk(dr, "202610221530-kx3f-abandoned-3", "202610221530-aaaa").unwrap();
    assert_eq!(r.new_name, "202610221530-aaaa-abandoned");
}

// ---- ids that no other branch picks ----

use lint_rules::round_id;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=dev",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn round_dir(root: &Path, name: &str, content: &str) {
    touch(
        &root.join("mock/design_rounds").join(name),
        ".meta",
        content,
    );
}

fn commit_all(root: &Path, msg: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", msg]);
}

fn cfg_in(root: &Path) -> Config {
    let mut c = Config::from_dir(Path::new("/nonexistent-mock-dir"));
    c.repo_root = root.to_path_buf();
    c.mock_dir = root.join("mock");
    c
}

/// A clone on `dev` whose `origin` has the round directories `on_dev` on its
/// `dev` and `on_main` on its `main`, each holding its own name as contents, and
/// fetched. The clone has no `origin/HEAD`, so a trunk is seen only by its name.
struct Clone {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    cfg:  Config,
}

fn clone_with(on_dev: &[&str], on_main: &[&str]) -> Clone {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let (up, root) = (base.join("up"), base.join("clone"));
    for dir in [&up, &root] {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q"]);
        touch(&dir.join("mock/design_rounds"), "README.md", "rounds\n");
        commit_all(dir, "chore: base");
    }
    git(&up, &["switch", "-q", "-c", "main"]);
    for name in on_main {
        round_dir(&up, name, name);
    }
    commit_all(&up, "chore: rounds on main");
    git(&up, &["switch", "-q", "dev"]);
    for name in on_dev {
        round_dir(&up, name, name);
    }
    commit_all(&up, "chore: rounds on dev");
    git(&root, &["remote", "add", "origin", up.to_str().unwrap()]);
    git(&root, &["fetch", "-q", "origin"]);
    let cfg = cfg_in(&root);
    Clone {
        _tmp: tmp,
        root,
        cfg,
    }
}

#[test]
fn the_control_a_clone_with_no_rounds_has_no_taken_ids() {
    // Without this a `taken_ids` that returned everything would pass the four below.
    let c = clone_with(&[], &[]);
    assert!(taken_ids(&c.cfg).is_empty());
}

#[test]
fn an_id_in_the_working_tree_is_taken() {
    // Mutant: the `design_rounds/` listing is dropped from `taken_ids`.
    let c = clone_with(&[], &[]);
    round_dir(&c.root, "202610221529-aaaa", "x");
    touch(
        &c.root.join("mock/design_rounds"),
        "202610221530-bbbb_topic.a.md",
        "x",
    );
    let taken = taken_ids(&c.cfg);
    assert!(
        taken.contains("202610221529-aaaa") && taken.contains("202610221530-bbbb"),
        "{taken:?}"
    );
}

#[test]
fn an_id_only_a_round_tag_holds_is_taken() {
    // Mutant: the `round/*` tag scan is dropped from `taken_ids`.
    let c = clone_with(&[], &[]);
    git(&c.root, &["tag", "round/202610221529-aaaa/end"]);
    git(&c.root, &[
        "tag",
        "round/202610221530-bbbb-abandoned/abandoned",
    ]);
    git(&c.root, &["tag", "v1.0.0"]);
    let taken = taken_ids(&c.cfg);
    assert!(
        taken.contains("202610221529-aaaa") && taken.contains("202610221530-bbbb"),
        "{taken:?}"
    );
    assert_eq!(
        taken.len(),
        3,
        "the ids and names of the round tags, and nothing from `v1.0.0`: {taken:?}"
    );
}

#[test]
fn an_id_only_origin_dev_holds_is_taken() {
    // Mutant: `origin/dev` is dropped from the trunks `taken_ids` lists.
    let c = clone_with(&["202610221529-aaaa"], &["202610221530-bbbb"]);
    let taken = taken_ids(&c.cfg);
    assert!(taken.contains("202610221529-aaaa"), "{taken:?}");
}

#[test]
fn an_id_only_origin_main_holds_is_taken() {
    // Mutant: `origin/main` is dropped from the trunks `taken_ids` lists.
    let c = clone_with(&["202610221529-aaaa"], &["202610221530-bbbb"]);
    let taken = taken_ids(&c.cfg);
    assert!(taken.contains("202610221530-bbbb"), "{taken:?}");
}

#[test]
fn a_fresh_id_walks_past_every_place_an_id_can_be_taken() {
    // The walk end to end: whichever place holds the first candidate, the id
    // that comes out is a different one. Mutant: any of the four scans dropped.
    let stamp = "202610221529";
    let seed = "branch|host|1|2";
    let first = round_id::fresh(stamp, seed, |_| false);
    for place in ["tree", "tag", "dev", "main"] {
        let c = match place {
            "dev" => clone_with(&[&first], &[]),
            "main" => clone_with(&[], &[&first]),
            _ => clone_with(&[], &[]),
        };
        match place {
            "tree" => round_dir(&c.root, &first, "x"),
            "tag" => {
                git(&c.root, &["tag", &format!("round/{first}/end")]);
            },
            _ => {},
        }
        let taken = taken_ids(&c.cfg);
        let next = round_id::fresh(stamp, seed, |id| taken.contains(id));
        assert_ne!(
            next, first,
            "{place} holds {first} and the walk did not leave it"
        );
    }
}

fn origin(branch: &str, host: &str, pid: u32, nanos: u128) -> Origin {
    Origin {
        branch: branch.into(),
        host: host.into(),
        pid,
        nanos,
    }
}

fn tag_of(o: &Origin) -> String {
    round_id::tag_for(&o.seed(), 0)
}

#[test]
fn a_different_branch_host_pid_or_instant_draws_a_different_tag() {
    // Mutant: `seed` is a constant, or drops one of the four. The tags are
    // deterministic, so these particular values either differ or never will.
    let here = origin("feat/a", "host-1", 4242, 1_000);
    for (what, other) in [
        ("branch", origin("feat/b", "host-1", 4242, 1_000)),
        ("host", origin("feat/a", "host-2", 4242, 1_000)),
        ("pid", origin("feat/a", "host-1", 4243, 1_000)),
        ("nanos", origin("feat/a", "host-1", 4242, 1_001)),
    ] {
        assert_ne!(here.seed(), other.seed(), "{what}");
        assert_ne!(
            tag_of(&here),
            tag_of(&other),
            "a different {what} drew the same tag"
        );
    }
    assert_eq!(
        tag_of(&here),
        tag_of(&here.clone()),
        "the same origin is the same tag"
    );
}

#[test]
fn the_seed_a_command_draws_from_is_the_clones_branch_and_the_processs_pid() {
    // Mutant: `Origin::here` or `seed` returns a constant, which was the original
    // defect: two sessions drawing one id.
    let c = clone_with(&[], &[]);
    git(&c.root, &["switch", "-q", "-c", "feat/one"]);
    let one = Origin::here(&c.cfg);
    assert_eq!(one.branch, "feat/one");
    assert_eq!(one.pid, std::process::id());
    assert!(one.nanos > 0);
    git(&c.root, &["switch", "-q", "-c", "feat/two"]);
    let two = Origin::here(&c.cfg);
    assert_eq!(two.branch, "feat/two");
    assert_ne!(one.seed(), two.seed());
    let said = seed(&c.cfg);
    assert!(
        said.starts_with("feat/two|") && said.contains(&format!("|{}|", std::process::id())),
        "{said}"
    );
}

#[test]
fn the_host_is_the_environments_when_it_has_one() {
    // Mutant: the command wins over the environment, or the host is dropped.
    assert_eq!(
        pick_host(Some("env-host".into()), || Some("cmd-host".into())),
        "env-host"
    );
    assert_eq!(pick_host(None, || Some("cmd-host\n".into())), "cmd-host\n");
    assert_eq!(pick_host(None, || None), "");
}

#[test]
fn a_round_that_has_landed_is_not_renamed() {
    // Mutant: `landed_on` is dropped from `cmd_rename_round`, or compares the
    // name and not the contents.
    let c = clone_with(&["202610221529"], &[]);
    round_dir(&c.root, "202610221529", "202610221529");
    commit_all(&c.root, "chore: close the round");
    assert_eq!(landed_on(&c.cfg, "202610221529"), Some("origin/dev"));
    cmd_rename_round(
        &c.cfg,
        &SubcmdOpts {
            auto_commit: false,
        },
        &["202610221529"],
    );
    let rounds = c.root.join("mock/design_rounds");
    assert_eq!(
        entries(&rounds),
        ["202610221529", "README.md"],
        "a landed round moved"
    );
}

#[test]
fn a_round_of_the_same_id_with_other_contents_is_the_one_a_rename_is_for() {
    // The case rename-round exists for: the trunk's round took the id first and
    // this branch closed another under it. Mutant: `landed_on` matches by name.
    let c = clone_with(&["202610221529"], &["202610221529"]);
    round_dir(&c.root, "202610221529", "closed on this branch");
    commit_all(&c.root, "chore: close another round");
    assert_eq!(landed_on(&c.cfg, "202610221529"), None);
    cmd_rename_round(
        &c.cfg,
        &SubcmdOpts {
            auto_commit: false,
        },
        &["202610221529"],
    );
    let renamed = entries(&c.root.join("mock/design_rounds"));
    assert_eq!(renamed.len(), 2, "{renamed:?}");
    assert!(
        renamed[0].starts_with("202610221529-") && renamed[1] == "README.md",
        "{renamed:?}"
    );
}

#[test]
fn a_round_landed_on_main_is_refused_too() {
    // Mutant: only `origin/dev` is asked.
    let c = clone_with(&[], &["202610221529"]);
    round_dir(&c.root, "202610221529", "202610221529");
    commit_all(&c.root, "chore: close the round");
    assert_eq!(landed_on(&c.cfg, "202610221529"), Some("origin/main"));
}

#[test]
fn a_round_the_trunks_do_not_hold_has_not_landed() {
    let c = clone_with(&["202610221500"], &[]);
    round_dir(&c.root, "202610221529", "x");
    commit_all(&c.root, "chore: close the round");
    assert_eq!(landed_on(&c.cfg, "202610221529"), None);
    assert_eq!(
        landed_on(&c.cfg, "202610221400"),
        None,
        "a round nobody has"
    );
}

/// Every name in `dir`, directories included, sorted.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}
