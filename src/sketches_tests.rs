//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

#[test]
fn the_outcome_line_is_read_in_the_shapes_the_skill_and_the_tree_use() {
    assert_eq!(outcome_of("# x\n\n**Outcome:** WORKS\n"), Outcome::Works);
    assert_eq!(outcome_of("**Outcome:** FAILS WITH E0277\n"), Outcome::Fails);
    assert_eq!(outcome_of("Outcome: inconclusive, the gate\n"), Outcome::Inconclusive);
    assert_eq!(outcome_of("- **Outcome**: `WORKS`\n"), Outcome::Works);
    assert_eq!(outcome_of("## Outcome: WORKS\n"), Outcome::Works);
    assert_eq!(outcome_of("## OUTCOME: fails with E0277\n"), Outcome::Fails);
    // The controls: a heading that is not an outcome line, a heading with no
    // verdict, a line naming no outcome, and no line at all.
    assert_eq!(outcome_of("## Result: WORKS\n"), Outcome::NoLine);
    assert_eq!(outcome_of("## OUTCOME\n\nWORKS\n"), Outcome::Works);
    assert!(matches!(outcome_of("## Outcome\n\nsee below\n"), Outcome::Unclear(_)));
    assert!(matches!(outcome_of("## Outcome\n"), Outcome::Unclear(_)));
    assert!(matches!(outcome_of("**Outcome:** maybe\n"), Outcome::Unclear(_)));
    assert_eq!(outcome_of("# x\n\n## WORKS\n"), Outcome::NoLine);
}

#[test]
fn a_timer_in_a_sketch_is_seen_and_plain_code_is_not() {
    assert!(times_something("let t = std::time::Instant::now();"));
    assert!(times_something("let d = start.elapsed();"));
    assert!(!times_something("fn f() -> u32 { 4 }"));
}

#[test]
fn the_survey_reads_each_sketch_and_open_keeps_only_those_wanting_attention() {
    let root_guard = mockspace_manifest::scratch::tempdir().unwrap();
    let root = root_guard.path().to_path_buf();
    let write = |rel: &str, text: &str| {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    };
    write("202601010000_done/FINDINGS.md", "**Outcome:** WORKS\n");
    write("202601010000_done/src/lib.rs", "pub fn f() {}\n");
    write("202601010100_bare/src/main.rs", "fn main() {}\n");
    write("202601010200_timed/FINDINGS.md", "**Outcome:** WORKS\n");
    write("202601010200_timed/src/main.rs", "fn main() { let t = std::time::Instant::now(); }\n");
    // Build output is never read: a timer there is the compiler's, not the sketch's.
    write("202601010000_done/target/x.rs", "Instant::now");

    let all = survey(&root);
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].outcome, Outcome::Works);
    assert_eq!(all[0].timer, None, "target/ is skipped");
    assert_eq!(all[1].outcome, Outcome::NoFindings);
    assert_eq!(all[2].timer, Some(PathBuf::from("src/main.rs")));

    let open = render(&all, true);
    assert!(!open.contains("_done"), "a recorded, untimed sketch is not open:\n{open}");
    assert!(open.contains("_bare  no FINDINGS.md"), "{open}");
    assert!(open.contains("_timed  WORKS  times something"), "{open}");
    assert!(render(&all, false).contains("_done  WORKS"), "the full list keeps it");
}

#[test]
fn an_unknown_flag_fails_and_a_known_one_does_not() {
    let root_guard = mockspace_manifest::scratch::tempdir().unwrap();
    let root = root_guard.path().to_path_buf();
    let mut cfg = Config::from_dir(Path::new("/nonexistent-mock-dir"));
    cfg.mock_dir = root.join("mock");
    assert_eq!(cmd(&cfg, &["--bogus".to_string()]), ExitCode::FAILURE);
    // The control: the flag it takes, and no flag, both succeed with no sketches dir.
    assert_eq!(cmd(&cfg, &["--open".to_string()]), ExitCode::SUCCESS);
    assert_eq!(cmd(&cfg, &[]), ExitCode::SUCCESS);
}


