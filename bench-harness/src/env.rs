//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Environment metadata captured once per harness run.
//!
//! Recorded into each run's `meta.json` beside its samples so historical
//! comparisons can isolate hardware / toolchain / commit drift.
//! Round 1 ships the type and a stub collector that returns empty
//! strings; Round 3 wires up the real `sysctl` / `/proc/cpuinfo` /
//! `rustc --version` / `git rev-parse` calls.

use serde::{Deserialize, Serialize};

/// Environment metadata recorded once per [`crate::run`] invocation.
///
/// All free-form strings are emitted verbatim into the cache; consumers
/// that want structured queries should parse on the read side.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnvMeta {
    /// CPU brand string (`sysctl machdep.cpu.brand_string` on macOS,
    /// `model name` from `/proc/cpuinfo` on Linux). Empty when
    /// collection fails.
    pub cpu:          String,
    /// `uname -sr` output. Identifies kernel + version.
    pub os:           String,
    /// `rustc --version` of whatever `rustc` is on PATH when the harness runs.
    /// That is not necessarily the toolchain the variant cdylibs were built
    /// with: they may have been built earlier, or by a crate carrying its own
    /// `rust-toolchain.toml`.
    pub rustc:        String,
    /// Short git commit (`git rev-parse --short HEAD`) of the consumer, with a
    /// `-dirty` suffix when a build input of the bench tree, tracked or new, differs
    /// from it, as [`run_commit`] says. Empty when not in a git tree.
    /// Written to the run's `meta.json`, and the commit its summary rows carry.
    pub git_commit:   String,
    /// Unix timestamp at run start (seconds since epoch).
    pub timestamp:    u64,
    /// Hardware counter frequency in Hz; used by the harness to convert
    /// raw counter ticks into nanoseconds.
    pub counter_freq: u64,
    /// Coarse machine label (see [`machine_class`]), so timings from different
    /// hardware are never pooled. Carries no hostname, user, MAC or serial.
    pub machine_class: String,
    /// Logical CPUs available to this process; zero when unknown.
    pub cpus_online: u64,
    /// One-minute load average at collection; `None` when unreadable.
    pub load_1m: Option<f64>,
}

/// The facts a machine class is derived from. Nothing in here identifies a
/// machine: the CPU brand is reduced to a family token and never passed through.
#[derive(Clone, Copy, Debug)]
pub struct MachineFacts<'a> {
    /// `std::env::consts::OS`.
    pub os:        &'a str,
    /// `std::env::consts::ARCH`.
    pub arch:      &'a str,
    /// Logical CPUs; zero when unknown.
    pub cpus:      u64,
    /// Whether the process runs inside a container.
    pub container: bool,
    /// CPU brand string, free-form.
    pub cpu_brand: &'a str,
}

/// A coarse label such as `linux-x86_64-c16-container-amd`: os, arch, core-count
/// bucket (the next power of two, at least 2), container or bare, and cpu family.
///
/// Every part is drawn from a closed vocabulary or a bucket, so a hostname or
/// serial inside the brand string cannot reach the label.
#[must_use]
pub fn machine_class(f: &MachineFacts<'_>) -> String {
    let bucket = match f.cpus {
        0 => "cx".to_string(),
        n => format!("c{}", n.next_power_of_two().max(2)),
    };
    format!(
        "{}-{}-{}-{}-{}",
        token(f.os),
        token(f.arch),
        bucket,
        if f.container { "container" } else { "bare" },
        cpu_family(f.cpu_brand),
    )
}

/// The values `std::env::consts::OS` and `ARCH` take.
const KNOWN_TOKENS: [&str; 54] = [
    "linux", "macos", "windows", "freebsd", "netbsd", "openbsd", "dragonfly", "android",
    "ios", "tvos", "watchos", "solaris", "illumos", "haiku", "redox", "fuchsia", "wasi",
    "emscripten", "vxworks", "cygwin", "aix", "nto", "espidf", "horizon", "l4re", "hurd",
    "x86", "x86_64", "arm", "aarch64", "arm64ec", "loongarch64", "m68k", "csky", "mips",
    "mips64", "mips32r6", "mips64r6", "powerpc", "powerpc64", "riscv32", "riscv64", "s390x",
    "sparc", "sparc64", "hexagon", "wasm32", "wasm64", "xtensa", "avr", "msp430", "nvptx64",
    "bpf", "amdgpu",
];

/// The os or arch from a closed list, `other` for anything outside it, so free text
/// (a hostname inside an os string) cannot reach the label.
fn token(s: &str) -> &'static str {
    KNOWN_TOKENS.iter().copied().find(|k| *k == s).unwrap_or("other")
}

/// The CPU family from a brand string, from a closed list; `other` otherwise.
fn cpu_family(brand: &str) -> &'static str {
    let b = brand.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| b.contains(n));
    if has(&["apple"]) {
        "apple"
    } else if has(&["intel", "xeon"]) {
        "intel"
    } else if has(&["amd", "epyc", "ryzen"]) {
        "amd"
    } else if has(&["graviton", "neoverse", "arm"]) {
        "arm"
    } else {
        "other"
    }
}

/// Whether a machine class agrees with what an older meta recorded of the
/// machine that ran it: the `uname -sr` it kept as `os` and the CPU brand it
/// kept as `cpu`. The class's os token must be the one that `uname` name
/// means, where it means one this knows, and its cpu family the one the brand
/// reduces to. Used to check a class a person supplies for results whose meta
/// predates the class, so a wrong one is refused rather than written into the
/// path.
#[must_use]
pub fn class_agrees_with(class: &str, uname_os: &str, cpu_brand: &str) -> bool {
    let parts: Vec<&str> = class.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let os = match uname_os.split_whitespace().next().unwrap_or("") {
        "Darwin" => Some("macos"),
        "Linux" => Some("linux"),
        "FreeBSD" => Some("freebsd"),
        "NetBSD" => Some("netbsd"),
        "OpenBSD" => Some("openbsd"),
        _ => None,
    };
    os.is_none_or(|os| parts[0] == os) && (cpu_brand.is_empty() || parts[4] == cpu_family(cpu_brand))
}

/// The first field of `/proc/loadavg` (`0.52 0.58 0.59 1/467 12345`).
#[must_use]
pub fn parse_proc_loadavg(s: &str) -> Option<f64> {
    s.split_whitespace().next()?.parse().ok().filter(|v: &f64| v.is_finite())
}

/// The first number of `sysctl -n vm.loadavg` (`{ 1.52 1.60 1.71 }`).
#[must_use]
pub fn parse_sysctl_loadavg(s: &str) -> Option<f64> {
    s.split_whitespace()
        .find(|w| *w != "{")?
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite())
}

/// The one-minute load average now, or `None` where the platform gives none.
#[must_use]
pub fn load_1m() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("sysctl").args(["-n", "vm.loadavg"]).output().ok()?;
        parse_sysctl_loadavg(&String::from_utf8(out.stdout).ok()?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        parse_proc_loadavg(&std::fs::read_to_string("/proc/loadavg").ok()?)
    }
}

/// Whether the facts a process can read say it runs in a container.
#[must_use]
pub fn container_from(
    dockerenv: bool,
    containerenv: bool,
    systemd_container: &str,
    cgroup: &str,
) -> bool {
    dockerenv
        || containerenv
        || !systemd_container.trim().is_empty()
        || ["docker", "kubepods", "containerd", "lxc"].iter().any(|k| cgroup.contains(k))
}

fn in_container() -> bool {
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    container_from(
        std::path::Path::new("/.dockerenv").exists(),
        std::path::Path::new("/run/.containerenv").exists(),
        &read("/run/systemd/container"),
        &read("/proc/1/cgroup"),
    )
}

/// Collect environment metadata for the current process.
///
/// Calls `sysctl` (macOS) or reads `/proc/cpuinfo` (Linux) for the CPU
/// brand, `uname -sr` for the OS string, `rustc --version` for the
/// toolchain on PATH, `git rev-parse --short HEAD` for the commit, the
/// system clock for `timestamp`, and
/// [`mockspace_bench_core::counter::counter_frequency`] for the
/// counter frequency. Any individual collection step that fails leaves
/// the corresponding field empty (no panic).
pub fn collect_env_meta() -> EnvMeta {
    let cpu = collect_cpu();
    let cpus = std::thread::available_parallelism().map_or(0, |n| n.get() as u64);
    EnvMeta {
        machine_class: machine_class(&MachineFacts {
            os:        std::env::consts::OS,
            arch:      std::env::consts::ARCH,
            cpus,
            container: in_container(),
            cpu_brand: &cpu,
        }),
        cpus_online:   cpus,
        load_1m:       load_1m(),
        cpu,
        os:           collect_os(),
        rustc:        collect_rustc(),
        git_commit:   run_commit(),
        timestamp:    collect_timestamp(),
        counter_freq: mockspace_bench_core::counter::counter_frequency(),
    }
}

/// The short commit, with `-dirty` when a tracked file in the tree differs
/// from it. Empty outside a git tree.
///
/// Untracked files never count: a file git does not track is not part of the
/// commit, whether it is a run's own output or a generated file a repository
/// forgot to ignore. A bench run spawned by `mock bench run` takes the narrower mark the tool
/// hands it, [`run_commit`].
#[must_use]
pub fn collect_git_commit_marked() -> String {
    let mut c = collect_git_commit();
    if !c.is_empty() {
        let dirty = std::process::Command::new("git")
            .args(["status", "--porcelain", "--untracked-files=no"])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if dirty {
            c.push_str("-dirty");
        }
    }
    c
}

/// The environment variable `mock bench run` sets on the driver it spawns:
/// the commit the run measured, with `-dirty` only when a file, tracked or new, the
/// bench tree is built from differed from it. The tool judges that, since it
/// is what knows the tree's build inputs (the files under the bench tree less
/// its outputs, every crate a bench manifest reaches by path, and the
/// workspace's own manifests, lockfiles, toolchain pin and cargo
/// configuration).
pub const COMMIT_ENV: &str = "MOCKSPACE_BENCH_COMMIT";

/// The commit a bench run records: the one `mock bench run` handed the driver
/// in [`COMMIT_ENV`], or, for a driver run by hand, the tree's commit marked
/// dirty by any tracked change ([`collect_git_commit_marked`]).
#[must_use]
pub fn run_commit() -> String {
    std::env::var(COMMIT_ENV)
        .ok()
        .filter(|c| !c.trim().is_empty())
        .unwrap_or_else(collect_git_commit_marked)
}

/// Only what a test run's rows carry: machine class, cpus, load and the time.
///
/// Cheap by design. [`collect_env_meta`] also calibrates the hardware counter
/// (seconds of sleeping) and shells out to `rustc` and `git`, none of which a
/// test row uses, and a test run must not pay that.
#[must_use]
pub fn collect_run_env() -> EnvMeta {
    let cpu = collect_cpu();
    let cpus = std::thread::available_parallelism().map_or(0, |n| n.get() as u64);
    EnvMeta {
        machine_class: machine_class(&MachineFacts {
            os:        std::env::consts::OS,
            arch:      std::env::consts::ARCH,
            cpus,
            container: in_container(),
            cpu_brand: &cpu,
        }),
        cpus_online:   cpus,
        load_1m:       load_1m(),
        cpu,
        timestamp:     collect_timestamp(),
        ..EnvMeta::default()
    }
}

#[cfg(target_os = "macos")]
fn collect_cpu() -> String {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    out.trim().to_string()
}

#[cfg(not(target_os = "macos"))]
fn collect_cpu() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|v| v.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".into())
}

fn collect_os() -> String {
    let out = std::process::Command::new("uname")
        .args(["-sr"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    out.trim().to_string()
}

fn collect_rustc() -> String {
    let out = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    out.trim().to_string()
}

fn collect_git_commit() -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    out.trim().to_string()
}

fn collect_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(brand: &str) -> MachineFacts<'_> {
        MachineFacts { os: "linux", arch: "x86_64", cpus: 12, container: false, cpu_brand: brand }
    }

    #[test]
    fn the_class_is_os_arch_core_bucket_kind_and_family() {
        assert_eq!(machine_class(&facts("AMD EPYC 7B13")), "linux-x86_64-c16-bare-amd");
        let c = MachineFacts { container: true, cpus: 2, ..facts("Intel(R) Xeon(R)") };
        assert_eq!(machine_class(&c), "linux-x86_64-c2-container-intel");
    }

    /// A hostname riding in the brand string, or in os or arch, must not reach
    /// the label: the family comes from a closed list and the rest is tokenised.
    #[test]
    fn a_hostname_in_the_inputs_never_reaches_the_label() {
        let label = machine_class(&facts("Mystery CPU on build-host-7f3a.example.com"));
        assert_eq!(label, "linux-x86_64-c16-bare-other");
        let f = MachineFacts { os: "linux-ip-10-0-0-5", arch: "host-a1b2", ..facts("x") };
        assert_eq!(machine_class(&f), "other-other-c16-bare-other");
    }

    #[test]
    fn unknown_cores_get_their_own_bucket_rather_than_the_smallest() {
        let f = MachineFacts { cpus: 0, ..facts("x") };
        assert!(machine_class(&f).contains("-cx-"));
        let f = MachineFacts { cpus: 500, ..facts("x") };
        assert!(machine_class(&f).contains("-c512-"));
        let f = MachineFacts { cpus: 192, ..facts("x") };
        assert!(machine_class(&f).contains("-c256-"), "a big box is not pooled with a 64-core one");
    }

    #[test]
    fn both_load_formats_read_their_first_number_and_refuse_junk() {
        assert_eq!(parse_proc_loadavg("0.52 0.58 0.59 1/467 12345\n"), Some(0.52));
        assert_eq!(parse_sysctl_loadavg("{ 1.52 1.60 1.71 }\n"), Some(1.52));
        assert_eq!(parse_proc_loadavg(""), None);
        assert_eq!(parse_proc_loadavg("NaN 1 2"), None);
        assert_eq!(parse_sysctl_loadavg("{ }"), None);
    }

    /// Docker's marker file, podman's, systemd's `/run/systemd/container` (what this
    /// cloud container shows, with no `.dockerenv`) and a cgroup path each say so alone.
    #[test]
    fn each_container_signal_alone_makes_it_a_container() {
        assert!(!container_from(false, false, "\n", "0::/\n"));
        assert!(container_from(true, false, "", ""));
        assert!(container_from(false, true, "", ""));
        assert!(container_from(false, false, "docker\n", "0::/\n"));
        assert!(container_from(false, false, "", "12:cpu:/kubepods/x\n"));
    }

    #[test]
    fn an_arch_outside_the_x86_and_arm_families_is_not_pooled_with_them() {
        let f = MachineFacts { arch: "powerpc64", ..facts("x") };
        assert!(machine_class(&f).contains("-powerpc64-"));
    }

    /// A class handed in for an older meta is checked against what that meta
    /// kept: the M1 results carry `Darwin` and `Apple M1`, so a macOS Apple
    /// class agrees and a Linux one, or an Intel one, does not.
    #[test]
    fn a_supplied_class_must_agree_with_the_os_and_cpu_the_meta_kept() {
        assert!(class_agrees_with("macos-aarch64-c8-bare-apple", "Darwin 25.6.0", "Apple M1"));
        assert!(!class_agrees_with("linux-aarch64-c8-bare-apple", "Darwin 25.6.0", "Apple M1"));
        assert!(!class_agrees_with("macos-aarch64-c8-bare-intel", "Darwin 25.6.0", "Apple M1"));
        assert!(!class_agrees_with("macos-aarch64-c8-bare", "Darwin 25.6.0", "Apple M1"));
        assert!(class_agrees_with("linux-x86_64-c4-container-intel", "Linux 6.18", "Intel(R) Xeon(R)"));
    }

    /// Every label the harness can compute is a class the layout accepts as a
    /// directory name, over every os and arch token (and one outside the
    /// list), every cpu family, both container flags and every core bucket:
    /// zero and each power of two and the count just past it, up to 4097.
    #[test]
    fn every_label_the_harness_computes_is_a_class_the_layout_accepts() {
        let brands = ["Apple M1", "Intel(R) Xeon(R)", "AMD EPYC 7B13", "Neoverse-N1", "Mystery CPU"];
        let tokens: Vec<&str> = KNOWN_TOKENS.iter().copied().chain(["not-a-token"]).collect();
        let cpus: Vec<u64> = std::iter::once(0).chain((0 .. 13).flat_map(|k| [1u64 << k, (1u64 << k) + 1])).collect();
        let mut checked = 0usize;
        for os in &tokens {
            for arch in &tokens {
                for brand in brands {
                    for container in [false, true] {
                        for &n in &cpus {
                            let f = MachineFacts { os, arch, cpus: n, container, cpu_brand: brand };
                            let label = machine_class(&f);
                            assert!(mockspace_bench_layout::is_machine_class(&label), "{label}");
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 55 * 55 * 5 * 2 * 27);
    }
}
