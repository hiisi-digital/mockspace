//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

const C: &str = "linux-x86_64-c4-bare-intel";
const HOST: &str = "target_arch=\"x86_64\"\ntarget_vendor=\"unknown\"\ntarget_os=\"linux\"\ntarget_env=\"gnu\"\ntarget_abi=\"\"\ntarget_feature=\"sse2\"\ndebug_assertions\n";

fn with(extra: &str) -> String {
    format!("{HOST}{extra}")
}

/// The host's own target and features are the machine class alone; flags
/// that change no target or feature (a profile's debug assertions) change
/// nothing.
#[test]
fn the_host_s_own_build_is_the_machine_class() {
    let native = with("target_feature=\"avx2\"\n");
    assert_eq!(system(C, HOST, &HOST.replace("debug_assertions\n", ""), &native, Some("znver4")), C);
}

/// Features the host CPU has, as `target-cpu=native` gives them, name the CPU
/// they resolved to, so two CPUs of one class are two systems; other features
/// are named by their set; another target by itself.
#[test]
fn features_and_targets_are_named_and_native_names_its_cpu() {
    let native = with("target_feature=\"avx2\"\ntarget_feature=\"fma\"\n");
    assert_eq!(system(C, HOST, &native, &native, Some("znver4")), format!("{C}+cpu=znver4"));
    assert_ne!(
        system(C, HOST, &native, &native, Some("znver4")),
        system(C, HOST, &native, &native, Some("emeraldrapids")),
        "one class, two CPUs: two systems"
    );
    let some = system(C, HOST, &with("target_feature=\"avx2\"\n"), &native, Some("znver4"));
    assert!(some.starts_with(&format!("{C}+features=2:")), "{some}");
    let musl = HOST.replace("\"gnu\"", "\"musl\"");
    assert_eq!(system(C, HOST, &musl, &native, None), format!("{C}+target=x86_64-unknown-linux-musl"));
}

#[test]
fn the_native_cpu_is_read_off_the_target_cpus_list() {
    let list = "Available CPUs for this target:\n    native - Select the CPU of the current host (currently emeraldrapids).\n    x86-64 - ...\n";
    assert_eq!(host_cpu(list).as_deref(), Some("emeraldrapids"));
    assert_eq!(host_cpu("x86-64\n"), None);
}

/// Cargo is asked, so every place a flag can be set counts by cargo's own
/// precedence: a config file in the directory the run starts in, a target
/// table, `CARGO_TARGET_<TRIPLE>_RUSTFLAGS`, and `RUSTFLAGS`, which
/// `CARGO_ENCODED_RUSTFLAGS` overrides when set; `target-cpu=native` names
/// the CPU it resolved to.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn the_system_is_what_cargo_would_build_for() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let krate = dir.path().join("k");
    std::fs::create_dir_all(krate.join("src")).unwrap();
    std::fs::write(krate.join("Cargo.toml"), "[workspace]\n[package]\nname = \"k\"\nversion = \"0.0.0\"\n").unwrap();
    std::fs::write(krate.join("src/lib.rs"), "").unwrap();
    let manifest = krate.join("Cargo.toml");
    const TRIPLE: &str = "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS";
    let none: [(&str, Option<&str>); 4] = [
        ("RUSTFLAGS", None),
        ("CARGO_ENCODED_RUSTFLAGS", None),
        ("CARGO_BUILD_RUSTFLAGS", None),
        (TRIPLE, None),
    ];
    let with = |extra: &[(&'static str, Option<&'static str>)]| {
        let mut v = none.to_vec();
        v.extend_from_slice(extra);
        v
    };
    let plain = probe_in(C, &manifest, &[], dir.path(), &none).unwrap();
    assert_eq!(plain, C, "the control: no flags anywhere");

    let cfg = dir.path().join(".cargo");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.toml"), "[build]\nrustflags = [\"-C\", \"target-feature=+avx2\"]\n").unwrap();
    let from_file = probe_in(C, &manifest, &[], dir.path(), &none).unwrap();
    assert!(from_file.starts_with(&format!("{C}+features=")), "a config file's flag counts: {from_file}");
    assert_eq!(
        probe_in(C, &manifest, &[], dir.path(), &with(&[("RUSTFLAGS", Some(""))])).unwrap(),
        C,
        "an empty RUSTFLAGS is what cargo applies over the file, and it is read so"
    );
    std::fs::write(cfg.join("config.toml"), "[target.x86_64-unknown-linux-gnu]\nrustflags = [\"-C\", \"target-feature=+avx2\"]\n").unwrap();
    assert_eq!(probe_in(C, &manifest, &[], dir.path(), &none).unwrap(), from_file, "a target table in a file");
    std::fs::remove_file(cfg.join("config.toml")).unwrap();

    let table = vec!["--config".to_string(), "target.x86_64-unknown-linux-gnu.rustflags=[\"-C\",\"target-feature=+avx2\"]".to_string()];
    assert_eq!(probe_in(C, &manifest, &table, dir.path(), &none).unwrap(), from_file);
    assert_eq!(
        probe_in(C, &manifest, &[], dir.path(), &with(&[(TRIPLE, Some("-C target-feature=+avx2"))])).unwrap(),
        from_file,
        "the triple's own variable"
    );

    let overridden = with(&[("RUSTFLAGS", Some("-C target-feature=+avx2")), ("CARGO_ENCODED_RUSTFLAGS", Some(""))]);
    assert_eq!(probe_in(C, &manifest, &[], dir.path(), &overridden).unwrap(), C, "RUSTFLAGS that cargo ignores is not read");
    let applied = probe_in(C, &manifest, &[], dir.path(), &with(&[("RUSTFLAGS", Some("-C target-feature=+avx2"))])).unwrap();
    assert_eq!(applied, from_file, "RUSTFLAGS cargo applies is read");

    let native = probe_in(C, &manifest, &[], dir.path(), &with(&[("RUSTFLAGS", Some("-C target-cpu=native"))])).unwrap();
    assert!(native.starts_with(&format!("{C}+cpu=")) && !native.ends_with("=native"), "native names its CPU: {native}");
}
