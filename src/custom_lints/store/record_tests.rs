use super::*;

#[test]
fn the_crate_is_tested_only_by_the_worktree_that_last_wrote_it() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("shared").join("k");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(gen_dir.join("Cargo.toml"), "").unwrap();
    let one = tmp.path().join("one").join("mock");
    let two = tmp.path().join("two").join("mock");
    assert_eq!(
        generated_dir_of(&one),
        Generated::Absent,
        "nothing generated yet"
    );
    record_owner(&gen_dir, &one, None).unwrap();
    assert_eq!(generated_dir_of(&one), Generated::Yours(gen_dir.clone()));
    record_owner(&gen_dir, &two, None).unwrap();
    assert_eq!(generated_dir_of(&two), Generated::Yours(gen_dir.clone()));
    assert_eq!(
        generated_dir_of(&one),
        Generated::Taken {
            dir:   gen_dir.clone(),
            owner: two.display().to_string(),
        },
        "rewritten by the other worktree since, which is not the same as never written"
    );
    // and a crate that was removed is absent, not taken
    std::fs::remove_dir_all(&gen_dir).unwrap();
    assert_eq!(generated_dir_of(&two), Generated::Absent);
}

#[test]
fn a_scoped_build_never_moves_the_pointer_the_test_run_reads() {
    // `mock test` takes the lints tree from the pointer. A one-tool crate behind it has no lint modules, so
    // the run would be green over nothing, the outcome entry/test.rs refuses. Mutation: the scoped build
    // records the pointer like the whole one, and the second assertion names the one-tool directory.
    let tmp = tempfile::tempdir().unwrap();
    let whole = tmp.path().join("shared").join("whole");
    let scoped = tmp.path().join("shared").join("scoped");
    for d in [&whole, &scoped] {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("Cargo.toml"), "").unwrap();
    }
    let mock = tmp.path().join("one").join("mock");
    record_owner(&whole, &mock, None).unwrap();
    record_owner(&scoped, &mock, Some("ticket-add")).unwrap();
    assert_eq!(
        generated_dir_of(&mock),
        Generated::Yours(whole.clone()),
        "the tests still run the whole build's lints"
    );
    // the control: the scoped directory is still owned and stamped, so the reaper and a later build see it
    assert_eq!(
        std::fs::read_to_string(scoped.join(OWNER)).unwrap(),
        mock.display().to_string()
    );
}

#[test]
fn a_whole_build_records_its_duplicate_tool_names_beside_the_pointer() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    assert_eq!(duplicates_of(&mock), None, "nothing is recorded before a whole build");
    record_duplicates(&mock, &["audit".to_string(), "sweep".to_string()]).unwrap();
    assert_eq!(duplicates_of(&mock), Some(vec!["audit".to_string(), "sweep".to_string()]));
    // the control: a whole build with none is a record of none, which is not the absent file
    record_duplicates(&mock, &[]).unwrap();
    assert_eq!(duplicates_of(&mock), Some(Vec::new()));
}

#[test]
fn a_scoped_run_refuses_a_recorded_duplicate_and_builds_the_rest_alone() {
    let rec = Some(vec!["audit".to_string()]);
    assert_eq!(plan_scoped(rec.clone(), "audit"), ScopedPlan::Refuse(vec!["audit".to_string()]));
    assert_eq!(plan_scoped(rec, "ticket-add"), ScopedPlan::OneTool);
    assert_eq!(plan_scoped(Some(Vec::new()), "audit"), ScopedPlan::OneTool);
}

#[test]
fn a_scoped_run_with_no_record_takes_the_whole_build_once() {
    assert_eq!(plan_scoped(None, "ticket-add"), ScopedPlan::Whole);
}
