//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a save writes and removes on disk: one file per member per flavour,
//! loaded back whole, and nothing touched that the run did not cover.

use super::*;

const T: f64 = 10.0;

fn case(bin: &str, name: &str, secs: f64, outcome: Outcome) -> Case {
    Case {
        binary: bin.to_string(),
        name: name.to_string(),
        secs,
        outcome,
    }
}

fn fps() -> BTreeMap<String, String> {
    BTreeMap::from([("render".to_string(), "fp1".to_string())])
}

/// A save by a run in `flavour` that covered `members`, in a workspace of just
/// them.
fn save_in(h: &History, mock: &Path, flavour: &str, members: &[&str]) -> Result<(), String> {
    let members: Vec<String> = members.iter().map(|m| m.to_string()).collect();
    h.save(mock, &Scope {
        flavour,
        ran: &members,
        workspace: &members,
    })
}

fn save(h: &History, mock: &Path, members: &[&str]) -> Result<(), String> {
    save_in(h, mock, "", members)
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "mockspace-suite-hist-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn live(entries: &[(&str, &[&str])]) -> crate::suite::live::Live {
    entries
        .iter()
        .map(|(b, ts)| (b.to_string(), ts.iter().map(|t| t.to_string()).collect()))
        .collect()
}

fn remembered(h: &mut History, bin: &str, name: &str) {
    h.record_cases("", &[case(bin, name, 40.0, Outcome::Pass)], &fps(), T);
}

/// The record is tracked files: one per member per flavour, loaded back whole.
#[test]
fn a_saved_history_loads_back_from_one_file_per_member_and_flavour() {
    let mock = scratch("roundtrip");
    let mut h = History::load(&mock);
    assert_eq!(h.version, VERSION);
    h.record_cases(
        "",
        &[case("render", "drawn", 40.0, Outcome::Pass)],
        &fps(),
        T,
    );
    h.record_cases(
        "--features editor",
        &[case("web::it", "drawn", 30.0, Outcome::Pass)],
        &fps(),
        T,
    );
    h.flavour_mut("").green.insert("mesh".to_string(), Green {
        full: "m1".to_string(),
        own:  BTreeMap::from([("mesh".to_string(), "o1".to_string())]),
    });
    let all = names_of(&["render", "mesh", "web"]);
    for flavour in ["", "--features editor"] {
        h.save(&mock, &Scope {
            flavour,
            ran: &all,
            workspace: &all,
        })
        .unwrap();
    }

    let dir = History::dir(&mock);
    for f in ["default/render.json", "default/mesh.json", "features-editor/web.json"] {
        assert!(dir.join(f).is_file(), "{f} was not written");
    }
    assert_eq!(History::load(&mock).flavours, h.flavours);
    std::fs::remove_dir_all(&mock).ok();
}

/// A run that settled nothing leaves every file as it was, so a rerun does not
/// show up as a change to commit.
#[test]
fn saving_what_is_already_there_writes_nothing() {
    let mock = scratch("still");
    let mut h = History::default();
    h.record_cases(
        "",
        &[case("render", "drawn", 40.0, Outcome::Pass)],
        &fps(),
        T,
    );
    save(&h, &mock, &["render"]).unwrap();
    let file = History::dir(&mock).join("default/render.json");
    let before = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut again = History::load(&mock);
    again.record_cases(
        "",
        &[case("render", "drawn", 41.0, Outcome::Pass)],
        &fps(),
        T,
    );
    save(&again, &mock, &["render"]).unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        before
    );
    std::fs::remove_dir_all(&mock).ok();
}

/// A member whose green a failure took away must not load it back from the
/// file it was saved in.
#[test]
fn a_member_with_nothing_left_loses_its_file() {
    let mock = scratch("stale");
    let mut h = History::default();
    h.flavour_mut("")
        .green
        .insert("mesh".to_string(), Green::default());
    save(&h, &mock, &["mesh"]).unwrap();
    h.flavour_mut("").green.remove("mesh");
    save(&h, &mock, &["mesh"]).unwrap();
    assert!(
        History::load(&mock)
            .flavour("")
            .is_none_or(|f| f.green.is_empty())
    );
    std::fs::remove_dir_all(&mock).ok();
}

#[test]
fn a_file_of_another_version_or_none_is_passed_over() {
    let mock = scratch("foreign");
    let dir = History::dir(&mock).join("default");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.json"),
        r#"{"version": 999, "flavour": "", "member": "a"}"#,
    )
    .unwrap();
    std::fs::write(dir.join("b.json"), "not json").unwrap();
    assert!(History::load(&mock).flavours.is_empty());
    std::fs::remove_dir_all(&mock).ok();
}

#[test]
fn the_pruned_record_is_what_a_save_leaves_on_disk() {
    let dir = scratch("prune-save");
    let mut h = History::default();
    remembered(&mut h, "village", "drawn");
    remembered(&mut h, "village::example/figure_preview", "draws");
    save(&h, &dir, &["village"]).unwrap();
    h.prune(
        "",
        &["village".to_string()],
        &live(&[("village", &["drawn"])]),
    );
    save(&h, &dir, &["village"]).unwrap();
    let text = std::fs::read_to_string(dir.join("test-history/default/village.json")).unwrap();
    assert!(
        text.contains("drawn") && !text.contains("figure_preview"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A pass is kept per test binary, in the file of the member it belongs to,
/// and comes back from it.
#[test]
fn the_binaries_that_passed_are_kept_in_their_members_files() {
    let mock = scratch("bins");
    let mut h = History::default();
    let f = h.flavour_mut("");
    f.bins.insert("render".to_string(), "base1".to_string());
    f.bins.insert("render::it".to_string(), "it1".to_string());
    f.bins
        .insert("web::sound_gate".to_string(), "sg1".to_string());
    save(&h, &mock, &["render", "web"]).unwrap();

    let dir = History::dir(&mock).join("default");
    let render = std::fs::read_to_string(dir.join("render.json")).unwrap();
    assert!(
        render.contains("render::it") && !render.contains("sound_gate"),
        "{render}"
    );
    assert!(dir.join("web.json").is_file());
    assert_eq!(History::load(&mock).flavours, h.flavours);

    // A binary whose record is dropped is dropped from its file, and a member
    // left with nothing loses the file.
    let f = h.flavour_mut("");
    f.bins.remove("web::sound_gate");
    f.bins.remove("render::it");
    save(&h, &mock, &["render", "web"]).unwrap();
    assert!(!dir.join("web.json").exists());
    assert_eq!(History::load(&mock).flavours[""].bins.len(), 1);
    std::fs::remove_dir_all(&mock).ok();
}

fn names_of(members: &[&str]) -> Vec<String> {
    members.iter().map(|m| m.to_string()).collect()
}

/// A save is for the members the run covered. The files of the others stand as
/// they are whatever they hold: blank, of another version, or not a record.
#[test]
fn a_save_leaves_the_files_of_members_that_did_not_run() {
    let mock = scratch("not-ours");
    let dir = History::dir(&mock).join("default");
    std::fs::create_dir_all(&dir).unwrap();
    let files = [
        (
            "blank",
            r#"{"version": 2, "flavour": "", "member": "blank", "green": null, "tests": {}, "bins": {}}"#,
        ),
        (
            "newer",
            r#"{"version": 3, "flavour": "", "member": "newer", "green": null, "tests": {}}"#,
        ),
        (
            "older",
            r#"{"version": 1, "flavour": "", "member": "older", "green": {"full": "x", "own": {}}, "tests": {}}"#,
        ),
        ("half", "<<<<<<< ours\n{}\n=======\n{}\n>>>>>>> theirs\n"),
    ];
    for (m, text) in files {
        std::fs::write(dir.join(format!("{m}.json")), text).unwrap();
    }
    let mut h = History::load(&mock);
    remembered(&mut h, "render", "drawn");
    let ran = names_of(&["render"]);
    let workspace = names_of(&["render", "blank", "newer", "older", "half"]);
    h.save(&mock, &Scope {
        flavour:   "",
        ran:       &ran,
        workspace: &workspace,
    })
    .unwrap();
    for (m, text) in files {
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{m}.json")))
                .ok()
                .as_deref(),
            Some(text),
            "{m}"
        );
    }
    assert!(dir.join("render.json").is_file());
    std::fs::remove_dir_all(&mock).ok();
}

/// What a member that is not in the workspace holds is no one's, and a save
/// takes it away in every flavour, whether or not the run loaded it.
#[test]
fn a_record_goes_when_its_member_does_and_not_before() {
    let mock = scratch("gone");
    let mut h = History::default();
    remembered(&mut h, "gone", "drawn");
    remembered(&mut h, "kept", "drawn");
    h.flavour_mut("--features editor")
        .green
        .insert("gone".to_string(), Green::default());
    let both = names_of(&["gone", "kept"]);
    for flavour in ["", "--features editor"] {
        h.save(&mock, &Scope {
            flavour,
            ran: &both,
            workspace: &both,
        })
        .unwrap();
    }
    let dir = History::dir(&mock);
    let gone = [dir.join("default/gone.json"), dir.join("features-editor/gone.json")];
    assert!(gone.iter().all(|p| p.is_file()));

    let h = History::load(&mock);
    let ran = names_of(&["unrelated"]);
    let workspace = names_of(&["kept", "unrelated"]);
    h.save(&mock, &Scope {
        flavour:   "",
        ran:       &ran,
        workspace: &workspace,
    })
    .unwrap();
    assert!(gone.iter().all(|p| !p.exists()), "its member is gone");
    assert!(dir.join("default/kept.json").is_file(), "its member is not");
    std::fs::remove_dir_all(&mock).ok();
}

/// A run is made in one flavour. Its member's files in the other flavours are
/// records of other runs: none is written, rewritten or removed, whatever the
/// run's memory holds of them.
#[test]
fn a_run_in_one_flavour_leaves_its_members_files_in_the_others() {
    let mock = scratch("other-flavours");
    let other = History::dir(&mock).join("features-editor");
    std::fs::create_dir_all(&other).unwrap();
    let older = r#"{"version": 1, "flavour": "--features editor", "member": "render", "green": {"full": "x", "own": {}}, "tests": {"render": {"drawn": {"secs": 40.0, "passed": true, "passed_at": "x"}}}}"#;
    std::fs::write(other.join("render.json"), older).unwrap();
    let blank = r#"{"version": 2, "flavour": "--release", "member": "render", "green": null, "tests": {}, "bins": {}}"#;
    let release = History::dir(&mock).join("release");
    std::fs::create_dir_all(&release).unwrap();
    std::fs::write(release.join("render.json"), blank).unwrap();

    let mut h = History::load(&mock);
    remembered(&mut h, "render", "drawn");
    let ran = names_of(&["render"]);
    h.save(&mock, &Scope {
        flavour:   "",
        ran:       &ran,
        workspace: &ran,
    })
    .unwrap();
    let kept = |d: &Path| std::fs::read_to_string(d.join("render.json")).ok();
    assert_eq!(kept(&other).as_deref(), Some(older), "the older record");
    assert_eq!(kept(&release).as_deref(), Some(blank), "the blank record");
    assert!(History::dir(&mock).join("default/render.json").is_file());
    std::fs::remove_dir_all(&mock).ok();
}

/// A member the workspace no longer has loses its file in every flavour's
/// directory, including one where nothing of it could be read.
#[test]
fn a_gone_members_unreadable_file_goes_too() {
    let mock = scratch("gone-unread");
    let dir = History::dir(&mock).join("features-editor");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("gone.json"), "<<<<<<< ours\n").unwrap();
    std::fs::write(dir.join("kept.json"), "<<<<<<< ours\n").unwrap();
    let ran = names_of(&["render"]);
    let workspace = names_of(&["render", "kept"]);
    History::default()
        .save(&mock, &Scope {
            flavour:   "",
            ran:       &ran,
            workspace: &workspace,
        })
        .unwrap();
    assert!(!dir.join("gone.json").exists());
    assert!(dir.join("kept.json").exists());
    std::fs::remove_dir_all(&mock).ok();
}
