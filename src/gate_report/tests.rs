use super::*;

fn f(sev: &str, kind: &str, n: usize) -> Finding {
    Finding {
        severity: sev.into(),
        kind:     kind.into(),
        location: format!("c/src/a.rs:{n}"),
        message:  format!("m{n}"),
    }
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gate-report-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn red_summary_is_a_line_per_kind_with_counts() {
    let findings = vec![
        f("error", "every-fixme-names-a-ticket", 1),
        f("error", "every-fixme-names-a-ticket", 2),
        f("error", "every-fixme-names-a-ticket", 3),
        f("error", "file-size", 4),
        f("warn", "no-todo", 5),
        f("warn", "no-todo", 6),
    ];
    let s = summary("pre-push", &findings, Path::new("/g/mockspace/pre-push-latest.log"));
    assert_eq!(
        s,
        "blocked: pre-push\n  every-fixme-names-a-ticket: 3\n  file-size: 1\n  (2 warnings in the log)\n  log: /g/mockspace/pre-push-latest.log\n"
    );
}

#[test]
fn warnings_are_never_listed_and_absent_when_there_are_none() {
    let findings = vec![f("error", "file-size", 1), f("warn", "no-todo", 2)];
    let s = summary("pre-commit", &findings, Path::new("/l"));
    assert!(!s.contains("no-todo"), "a warning's lint was printed: {s}");
    assert!(!s.contains("m2"), "a warning's message was printed: {s}");
    let none = summary("pre-commit", &[f("error", "file-size", 1)], Path::new("/l"));
    assert!(!none.contains("warnings"));
}

#[test]
fn a_line_round_trips_and_stays_on_one_line() {
    let finding = Finding {
        severity: "error".into(),
        kind:     "x".into(),
        location: "c/a.rs:3".into(),
        message:  "two\nlines\tand a tab".into(),
    };
    let line = finding.line();
    assert_eq!(line.lines().count(), 1);
    assert_eq!(line.split('\t').count(), 4, "{line}");
    let back = Finding::parse(&line).unwrap();
    assert_eq!(back.kind, "x");
    assert_eq!(back.location, "c/a.rs:3");
    assert_eq!(back.message, "two lines and a tab");
    assert!(Finding::parse("--- lint errors (3) ---").is_none());
}

#[test]
fn a_red_gate_with_no_lint_finding_is_summarised_from_its_blocked_lines() {
    let raw = "   Compiling x\nBLOCKED: deny check failed (a)\n";
    let all = collect("", raw);
    let s = summary("pre-push", &all, Path::new("/l"));
    assert!(s.contains("  deny: 1"), "{s}");
    let silent = collect("", "Compiling a\nthe last line\n");
    assert_eq!(silent[0].kind, "failed");
    assert_eq!(silent[0].message, "the last line");
}

#[test]
fn logs_hold_greppable_findings_the_raw_and_a_latest_link_and_are_pruned() {
    let dir = scratch("logs");
    for stamp in 1 ..= 8u64 {
        write_logs(&dir, "pre-push", stamp, &[f("error", "file-size", stamp as usize)], "RAW\n")
            .unwrap();
    }
    let latest = std::fs::read_to_string(dir.join("pre-push-latest.log")).unwrap();
    assert!(latest.starts_with("error\tfile-size\tc/src/a.rs:8\tm8"), "{latest}");
    assert_eq!(std::fs::read_to_string(dir.join("pre-push-latest.raw")).unwrap(), "RAW\n");
    let numbered = |ext: &str| {
        std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(ext) && !n.contains("latest"))
            .count()
    };
    assert_eq!(numbered(".log"), KEEP);
    assert_eq!(numbered(".raw"), KEEP);
}
