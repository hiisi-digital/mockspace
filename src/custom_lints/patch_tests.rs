use super::*;

#[test]
fn patch_section_points_at_the_checkout_path() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    // lay out cargo's checkout: git/checkouts/<name>-<hash>/<short-rev>/lint-rules
    let lr = home.join("git/checkouts/mockspace-deadbeef/abc1234/lint-rules");
    std::fs::create_dir_all(&lr).unwrap();
    let dep = "{ package = \"mockspace-lint-rules\", git = \"ssh://x/mockspace.git\", rev = \"abc1234ff\" }";
    let patch = patch_section(dep, home).unwrap();
    assert!(patch.contains("[patch.\"ssh://x/mockspace.git\"]"));
    assert!(patch.contains(&format!(
        "mockspace-lint-rules = {{ path = \"{}\" }}",
        lr.display()
    )));
}

#[test]
fn patch_section_covers_both_spellings_of_one_repository() {
    // The defect this is the fix for, and it is worth stating as a case
    // rather than trusting the helper below: cargo keys a source on the url
    // string, so a pack naming the repository by ssh while the engine names
    // it by https resolves to two copies of `mockspace-lint-rules`, whose
    // `LintPack` types do not unify. The cdylib then fails to build and
    // every lint under it stops running.
    //
    // Observed in `huurre`, which tracks the engine by branch and so rolled
    // onto a revision emitting the https form, while `mockspace-extra-lints`
    // and one of its own tools name the ssh form. The error named
    // `mockspace_lint_rules` twice and neither url once.
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let lr = home.join("git/checkouts/mockspace-deadbeef/b57007c/lint-rules");
    std::fs::create_dir_all(&lr).unwrap();

    let https = "{ package = \"mockspace-lint-rules\", git = \
                  \"https://github.com/hiisi-digital/mockspace.git\", rev = \"b57007c900\" }";
    let patch = patch_section(https, home).unwrap();
    assert!(
        patch.contains("[patch.\"https://github.com/hiisi-digital/mockspace.git\"]"),
        "the spelling the engine wrote is not patched: {patch}"
    );
    assert!(
        patch.contains("[patch.\"ssh://git@github.com/hiisi-digital/mockspace.git\"]"),
        "a pack naming the same repository by ssh is not covered: {patch}"
    );

    // And the same in the other direction, since which spelling the engine
    // carries is not this function's to assume.
    let ssh = "{ package = \"mockspace-lint-rules\", git = \
                \"ssh://git@github.com/hiisi-digital/mockspace.git\", rev = \"b57007c900\" }";
    let patch = patch_section(ssh, home).unwrap();
    assert!(patch.contains("[patch.\"https://github.com/hiisi-digital/mockspace.git\"]"));
    assert!(patch.contains("[patch.\"ssh://git@github.com/hiisi-digital/mockspace.git\"]"));
}

#[test]
fn a_url_in_neither_form_is_passed_through_unchanged() {
    // The boundary. A self-hosted forge, a `git://` url, a scheme nobody
    // here uses: one table, the url as written, exactly as before this
    // change. Guessing a second spelling for a host whose conventions are
    // unknown would emit a patch for a source that does not exist, and a
    // patch nothing matches is a manifest that no longer explains itself.
    assert_eq!(spellings_of("git://example.invalid/m.git").len(), 1);
    assert_eq!(spellings_of("ssh://x/mockspace.git").len(), 1);
    assert_eq!(spellings_of("file:///home/somebody/mockspace"), vec![
        "file:///home/somebody/mockspace".to_string()
    ]);
}

#[test]
fn exactly_these_two_spellings_and_no_others() {
    // What this pins that the two cases above do not: that the set is
    // exactly the pair, so a third form cannot be added without somebody
    // reading the reasoning for why there are two.
    //
    // It carried a false claim before, that a one-sided mapping would pass
    // both other cases while producing a wrong second spelling. It would
    // not: mapping https to both and ssh only to itself fails the coverage
    // case outright, since that case asserts the https table for ssh input.
    // Verified by mutation rather than by reading, which is how the claim
    // was found.
    let https = "https://github.com/hiisi-digital/mockspace.git";
    let ssh = "ssh://git@github.com/hiisi-digital/mockspace.git";
    assert_eq!(spellings_of(https), vec![
        https.to_string(),
        ssh.to_string()
    ]);
    assert_eq!(spellings_of(ssh), vec![https.to_string(), ssh.to_string()]);
}

#[test]
fn no_url_yields_the_same_spelling_twice() {
    // What this pins is that the dedup is still there, and not that a
    // colliding third form would be caught. With the dedup present a
    // collision is absorbed rather than reported, so adding a `.git`-less
    // spelling keeps this green; removing the dedup turns it red, naming
    // `https://github.com/o/r`. Established by mutation, both ways round.
    //
    // Absorbing is the right answer rather than a shortcut. Patch keys are
    // a set, so one table is the correct output for two rules naming one
    // source, and refusing at run time would turn a harmless redundancy
    // into a hard failure in a consumer's repository at commit time, which
    // is the cost this whole change exists to remove. The loudness belongs
    // here, at development time, and here is where it is.
    //
    // The dedup is also what makes the list below acceptable. Nine urls is
    // a sample over an unbounded space, and sampling a law is the shape the
    // test gate rejects; it is harmless only because an unsampled collision
    // cannot reach a consumer.
    //
    // What a collision would cost without it: two identical `[patch."X"]`
    // tables is `error: duplicate key`, raised at manifest parse before
    // anything resolves, so every `cargo mock` in that repository stops.
    for url in [
        "https://github.com/hiisi-digital/mockspace.git",
        "ssh://git@github.com/hiisi-digital/mockspace.git",
        "https://github.com/o/r",
        "ssh://git@host/r",
        "https://git@github.com/o/r.git",
        "git://example.invalid/m.git",
        "file:///home/somebody/mockspace",
        "ssh://x/mockspace.git",
        "",
    ] {
        let out = spellings_of(url);
        let mut sorted = out.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            out.len(),
            "{url:?} yields a repeated patch key: {out:?}"
        );
    }
}

#[test]
fn a_git_rev_dep_is_the_only_shape_worth_fetching_for() {
    // The guard on the retry. A dep with no rev, or none at all, has
    // nothing a patch could point at, so fetching for it buys a cargo
    // invocation per run and no patch either way.
    assert!(is_git_rev_dep(
        "{ package = \"mockspace-lint-rules\", git = \"ssh://x/m.git\", rev = \"abc\" }"
    ));
    assert!(!is_git_rev_dep(
        "{ package = \"mockspace-lint-rules\", git = \"ssh://x/m.git\", branch = \"dev\" }"
    ));
    assert!(!is_git_rev_dep("{ path = \"../lint-rules\" }"));
    assert!(!is_git_rev_dep("\"1.2.3\""));
    // A url with no scheme is one cargo refuses, so fetching against it
    // spends a process to be told so and leaves the error on screen.
    assert!(!is_git_rev_dep(
        "{ package = \"mockspace-lint-rules\", git = \"u\", rev = \"abc\" }"
    ));
}

#[test]
fn the_fetch_package_names_the_engine_dependency_and_nothing_else() {
    // What the retry fetches. It has to parse as a package on its own, or
    // the fetch that materialises the checkout cannot run at all and the
    // patch stays missing the same silent way it did before.
    let dep = "{ package = \"mockspace-lint-rules\", git = \"ssh://x/m.git\", rev = \"abc\" }";
    let m = fetch_manifest(dep);
    assert!(m.trim_start().starts_with("[workspace]"));
    assert!(m.contains("[lib]\npath = \"lib.rs\""));
    assert!(m.contains(&format!("mockspace = {dep}")));
    // One dependency and no other, so the fetch cannot drag in a pack or a
    // tool, and cannot fail on one that does not resolve.
    let deps = m.split("[dependencies]\n").nth(1).unwrap();
    assert_eq!(deps.lines().filter(|l| !l.trim().is_empty()).count(), 1);
}

/// A cold cargo home has no checkout, so the patch is missing; the fetch
/// materialises one and the same call then answers.
///
/// This is the whole of the defect: the manifest is written before the
/// generated crate is built, so on the first run in any repo the directory
/// [`find_lint_rules_checkout`] reads does not exist yet, `patch_section`
/// returns `None`, and nothing is emitted or said. The build then fails on
/// `E0308` between two `LintPack`s and the next run works, which reads as
/// flakiness rather than as an ordering bug.
///
/// The `None` before the fetch is the case that has to fail, and it is the
/// state every cold machine is in. A `file://` url keeps it off the
/// network, and `CARGO_HOME` is planted so the fetch and the read agree on
/// a root this test owns.
#[test]
fn a_cold_cargo_home_gets_its_checkout_from_the_fetch() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("engine");
    let home = tmp.path().join("cargo-home");
    let gen_dir = tmp.path().join("gen");
    std::fs::create_dir_all(repo.join("lint-rules/src")).unwrap();
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(
        repo.join("Cargo.toml"),
        "[workspace]\nmembers = [\"lint-rules\"]\nresolver = \"3\"\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("lint-rules/Cargo.toml"),
        "[package]\nname = \"mockspace-lint-rules\"\nversion = \"0.0.0\"\n\
         edition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(repo.join("lint-rules/src/lib.rs"), "").unwrap();

    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(["-C", repo.to_str().unwrap()])
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "git {args:?} failed");
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "e", "--no-verify"]);
    let rev = String::from_utf8(
        Command::new("git")
            .args(["-C", repo.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let dep = format!(
        "{{ package = \"mockspace-lint-rules\", git = \"file://{}\", rev = \"{}\" }}",
        repo.display(),
        rev.trim()
    );

    // The failing case, and the state of every machine that has not built
    // this repo before.
    assert!(
        patch_section(&dep, &home).is_none(),
        "a cargo home with no checkout must yield no patch"
    );

    ensure_lint_rules_checkout(&gen_dir, &dep, &home);

    let patch = patch_section(&dep, &home)
        .expect("the fetch must leave a checkout the patch can point at");
    // It points into the planted home rather than the machine's own, which
    // is what says the fetch honoured the `CARGO_HOME` it was passed.
    assert!(
        patch.contains(&home.join("git/checkouts").display().to_string()),
        "patch points outside the planted cargo home: {patch}"
    );
    assert!(patch.contains("mockspace-lint-rules = { path ="));
}

#[test]
fn patch_section_none_when_checkout_absent_or_not_git() {
    let tmp = tempfile::tempdir().unwrap();
    // git-rev dep but no checkout on disk -> no patch.
    let dep = "{ package = \"mockspace-lint-rules\", git = \"ssh://x/m.git\", rev = \"abc\" }";
    assert!(patch_section(dep, tmp.path()).is_none());
    // path/registry override -> no patch.
    assert!(patch_section("{ path = \"../lint-rules\" }", tmp.path()).is_none());
}
