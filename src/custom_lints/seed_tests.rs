use std::path::Path;
use std::process::Command;

use super::follow_consumer_lock;
use crate::custom_lints::identity::members_from_metadata;

fn lock_with(pin: &str) -> String {
    format!("version = 4\n\n[[package]]\nname = \"dep\"\nversion = \"0.1.0\"\nsource = \"git+ssh://git@example.com/o/dep?branch=dev#{pin}\"\n")
}

#[test]
fn the_consumers_pin_replaces_an_older_one_in_the_generated_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let (gen_dir, consumer) = (tmp.path().join("gen"), tmp.path().join("Cargo.lock"));
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(gen_dir.join("Cargo.lock"), lock_with("f700f75")).unwrap();
    std::fs::write(&consumer, lock_with("5dd2ab8f")).unwrap();
    follow_consumer_lock(&gen_dir, &consumer).unwrap();
    let now = std::fs::read_to_string(gen_dir.join("Cargo.lock")).unwrap();
    assert!(now.contains("#5dd2ab8f") && !now.contains("f700f75"), "{now}");
}

#[test]
fn a_fresh_generated_dir_is_given_the_consumers_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let (gen_dir, consumer) = (tmp.path().join("gen"), tmp.path().join("Cargo.lock"));
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(&consumer, lock_with("5dd2ab8f")).unwrap();
    follow_consumer_lock(&gen_dir, &consumer).unwrap();
    assert_eq!(std::fs::read_to_string(gen_dir.join("Cargo.lock")).unwrap(), lock_with("5dd2ab8f"));
}

#[test]
fn an_unmoved_consumer_lock_leaves_cargos_own_lock_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let (gen_dir, consumer) = (tmp.path().join("gen"), tmp.path().join("Cargo.lock"));
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(&consumer, lock_with("5dd2ab8f")).unwrap();
    follow_consumer_lock(&gen_dir, &consumer).unwrap();
    std::fs::write(gen_dir.join("Cargo.lock"), "trimmed by cargo\n").unwrap();
    follow_consumer_lock(&gen_dir, &consumer).unwrap();
    assert_eq!(std::fs::read_to_string(gen_dir.join("Cargo.lock")).unwrap(), "trimmed by cargo\n");
    std::fs::write(&consumer, lock_with("a978f46")).unwrap();
    follow_consumer_lock(&gen_dir, &consumer).unwrap();
    assert_eq!(std::fs::read_to_string(gen_dir.join("Cargo.lock")).unwrap(), lock_with("a978f46"));
}

#[test]
fn no_consumer_lock_keeps_what_the_generated_dir_has() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(gen_dir.join("Cargo.lock"), lock_with("f700f75")).unwrap();
    follow_consumer_lock(&gen_dir, &tmp.path().join("Cargo.lock")).unwrap();
    assert_eq!(std::fs::read_to_string(gen_dir.join("Cargo.lock")).unwrap(), lock_with("f700f75"));
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    follow_consumer_lock(&empty, &tmp.path().join("Cargo.lock")).unwrap();
    assert!(!empty.join("Cargo.lock").exists());
}

fn run(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(args[0])
        .args(&args[1 ..])
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(repo: &Path, version: &str) -> String {
    std::fs::write(
        repo.join("Cargo.toml"),
        format!("[package]\nname = \"dep\"\nversion = \"{version}\"\nedition = \"2024\"\n"),
    )
    .unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/lib.rs"), "").unwrap();
    run(repo, &["git", "add", "Cargo.toml", "src/lib.rs"]);
    run(repo, &["git", "commit", "-q", "-m", version]);
    run(repo, &["git", "rev-parse", "HEAD"])
}

fn dep_source(gen_dir: &Path) -> String {
    let out = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(gen_dir.join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let members = members_from_metadata(&json, &gen_dir.join("Cargo.toml")).unwrap();
    members.into_iter().find(|m| m.name == "dep").and_then(|m| m.source).unwrap()
}

/// A moved pin in the consumer's lock is a moved member source, which the identity digests, so the
/// kept library is not loaded for the new pin.
#[test]
fn a_moved_consumer_pin_moves_the_resolution_the_identity_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("dep");
    std::fs::create_dir_all(&repo).unwrap();
    run(&repo, &["git", "init", "-q", "-b", "dev"]);
    let first = commit(&repo, "0.1.0");
    let second = commit(&repo, "0.1.1");
    let url = format!("file://{}", repo.display());

    let gen_dir = tmp.path().join("gen");
    std::fs::create_dir_all(gen_dir.join("src")).unwrap();
    std::fs::write(gen_dir.join("src/lib.rs"), "").unwrap();
    std::fs::write(
        gen_dir.join("Cargo.toml"),
        format!("[workspace]\n\n[package]\nname = \"g\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\ndep = {{ git = \"{url}\", branch = \"dev\" }}\n"),
    )
    .unwrap();
    let lock = |rev: &str, version: &str| {
        format!("version = 4\n\n[[package]]\nname = \"dep\"\nversion = \"{version}\"\nsource = \"git+{url}?branch=dev#{rev}\"\n\n[[package]]\nname = \"g\"\nversion = \"0.0.0\"\ndependencies = [\"dep\"]\n")
    };
    let consumer = tmp.path().join("consumer.lock");

    // the directory was first built when the tip was the second commit
    std::fs::write(gen_dir.join("Cargo.lock"), lock(&second, "0.1.1")).unwrap();
    std::fs::write(&consumer, lock(&first, "0.1.0")).unwrap();
    follow_consumer_lock(&gen_dir, &consumer).unwrap();
    let pinned_first = dep_source(&gen_dir);
    assert!(pinned_first.ends_with(&first), "{pinned_first}");

    std::fs::write(&consumer, lock(&second, "0.1.1")).unwrap();
    follow_consumer_lock(&gen_dir, &consumer).unwrap();
    let pinned_second = dep_source(&gen_dir);
    assert!(pinned_second.ends_with(&second), "{pinned_second}");
    assert_ne!(pinned_first, pinned_second);
}
