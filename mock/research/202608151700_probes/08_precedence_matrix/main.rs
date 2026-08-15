//! Probe 08: the full precedence matrix of the bench tree's composition,
//! over both member forms, exercised rather than read.
//!
//! `compose_composed_member` (`tree.rs:444-570` on `origin/dev`) resolves a
//! sweep's declaration against its member's for eleven fields;
//! `compose_sections_member` (`tree.rs:378-443`) does the same for a
//! sections-form member; `merge_timing` (`tree.rs:606-623`) handles five
//! timing knobs; and `BenchManifest::for_size` applies the root's last. All
//! of it is hand-written `or` / `or_else` / `unwrap_or` chains. The sibling
//! derivation read this surface and explicitly conceded it was never
//! exercised. This exercises it.
//!
//! ## The shape, and why it is four cases rather than two
//!
//!   base  declared nowhere                 -> the documented default D
//!   lo    declared only at the lower level  -> L
//!   hi    declared only at the higher level -> H
//!   both  L below, H above                  -> H   (the precedence claim)
//!
//! NEGATIVE CONTROLS:
//!   C1  D, L and H must be pairwise distinct. An "override" equal to the
//!       default proves nothing. Booleans cannot satisfy this with two
//!       true values, so the boolean fields declare `false` at the higher
//!       level against `true` at the lower one.
//!   C2  `lo` must return L. If it returns D, the LOWER level is inert and
//!       "the higher level wins" is vacuously true while the lower level
//!       silently does nothing.
//!   C3  `hi` must return H, same argument for the upper level.
//!
//! C2 is the one that matters here, and it is not hypothetical: the
//! sections form shipped a defect of exactly that shape, where a member's
//! `[timing]` was read off `parsed.timing` (whose fields carry serde
//! defaults) so every undeclared knob was overridden with the framework
//! default. A two-case probe cannot see it. This probe is run against both
//! the pre-fix tree and the post-fix tree, and it must FAIL on the first.
//! That run is the instrument's own control and is committed beside this.

use std::fs;
use std::path::{Path, PathBuf};

use mockspace_bench_harness::config::BenchConfig;
use mockspace_bench_harness::tree;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, body).unwrap();
}

#[derive(Clone, Copy, PartialEq)]
enum Form {
    /// Top-level fields plus `[sweep.<name>]`, no wrapper table.
    Composed,
    /// `[bench.<name>]` sections inside a member directory.
    Sections,
}

/// Build a benchspace with one member and resolve its first point.
///
/// `root_extra`   goes in the root bench.toml
/// `member_extra` goes at the member's own top level (its [timing], etc.)
/// `inner_extra`  goes in the sweep (composed) or the section (sections)
/// `member_body`  extra top-level keys for the composed form only
fn resolve(
    form: Form,
    tag: &str,
    root_extra: &str,
    member_extra: &str,
    inner_extra: &str,
    member_body: &str,
) -> Result<BenchConfig, String> {
    let root = PathBuf::from(std::env::temp_dir())
        .join("precprobe")
        .join(tag);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    write(&root, "bench.toml", root_extra);

    match form {
        Form::Composed => {
            write(
                &root,
                "widths/bench.toml",
                &format!(
                    "{body}\n{member_extra}\n[sweep.s]\npoints = [1024]\n{inner_extra}\n",
                    body = if member_body.is_empty() {
                        "title = \"base title\"\narms = [\"packed\",\"dense\"]"
                    } else {
                        member_body
                    }
                ),
            );
            write(&root, "widths/arms/packed/src/lib.rs", "// arm\n");
            write(&root, "widths/arms/dense/src/lib.rs", "// arm\n");
        },
        Form::Sections => {
            // A sections-form member: its own [timing] plus a [bench.*]
            // section. Variants are paths, which is what this form carries.
            write(
                &root,
                "widths/bench.toml",
                &format!(
                    "{member_extra}\n[bench.s]\ntitle = \"base title\"\n\
                     workload = \"default\"\n\
                     variants = [\"variants/packed/target/release/libpacked.dylib\"]\n\
                     sizes = [1024]\n{inner_extra}\n"
                ),
            );
        },
    }

    let t = tree::load(&root).map_err(|e| first_line(&format!("{e}")))?;
    let key = ["widths/s", "widths", "s"]
        .into_iter()
        .find(|k| t.manifest.bench.contains_key(*k))
        .ok_or_else(|| {
            let mut ks: Vec<_> = t.manifest.bench.keys().cloned().collect();
            ks.sort();
            format!("no cell; keys={ks:?}")
        })?;
    t.manifest
        .for_size(key, 0, &root)
        .map_err(|e| first_line(&format!("{e}")))
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(70).collect()
}

struct Field {
    name:    &'static str,
    /// The member-level line present in the `base` and `hi` cases. Empty for
    /// every optional field; non-empty only for a field the form REQUIRES,
    /// where "declared nowhere" is not a reachable state and the base case
    /// must therefore declare it.
    base_lo: &'static str,
    lo:      &'static str,
    hi:      &'static str,
    default: &'static str,
    l:       &'static str,
    h:       &'static str,
    /// extra top-level body needed so `lo` does not duplicate a key the
    /// scaffold already writes
    body:    &'static str,
    read:    fn(&BenchConfig) -> String,
}

fn run_field(f: &Field, failures: &mut Vec<String>) {
    let g = |case: &str, m: &str, s: &str| -> String {
        let tag = format!("c-{}-{}", f.name, case);
        match resolve(Form::Composed, &tag, "", m, s, f.body) {
            Ok(c) => (f.read)(&c),
            Err(e) => format!("ERR:{e}"),
        }
    };
    let base = g("base", f.base_lo, "");
    let lo = g("lo", f.lo, "");
    let hi = g("hi", f.base_lo, f.hi);
    let both = g("both", f.lo, f.hi);

    let mut notes = Vec::new();
    // C1. For a field with more than two inhabitants, D, L and H must be
    // pairwise distinct. A bool has two, so H necessarily equals D; there
    // the control that matters is that `both` still returns H, which proves
    // an explicit `Some(false)` is distinguished from `None` rather than
    // swallowed by `unwrap_or`. That is the real hazard for `Option<bool>`.
    let boolean = f.default == "false" && f.l == "true" && f.h == "false";
    if !boolean && (f.l == f.default || f.h == f.default || f.l == f.h) {
        notes.push("C1 FAILED (D, L, H not distinct)".into());
    }
    if !base.starts_with(f.default) {
        notes.push(format!("default {base} != {}", f.default));
    }
    if lo != f.l {
        notes.push(format!("C2 member inert: {lo} != {}", f.l));
    }
    if hi != f.h {
        notes.push(format!("C3 sweep inert: {hi} != {}", f.h));
    }
    if both != f.h {
        notes.push(format!("PRECEDENCE: both={both}, want {}", f.h));
    }
    let v = if notes.is_empty() { "ok".to_string() } else { notes.join("; ") };
    println!(
        "  {:<13} base={:<12} lo={:<12} hi={:<12} both={:<12} {}",
        f.name, base, lo, hi, both, v
    );
    if !notes.is_empty() {
        failures.push(format!("composed/{}: {}", f.name, v));
    }
}

fn timing_matrix(form: Form, label: &str, failures: &mut Vec<String>) {
    println!();
    println!("== timing, {label}: sweep/section over member over root ==");
    let knobs: [(&str, fn(&BenchConfig) -> String); 5] = [
        ("passes", |c| c.passes.to_string()),
        ("runs_per_pass", |c| c.runs_per_pass.to_string()),
        ("batch_size", |c| c.batch_size.to_string()),
        ("harness_runs", |c| c.harness_runs.to_string()),
        ("cooldowns_ms", |c| format!("{:?}", c.cooldowns_ms)),
    ];
    for (knob, read) in knobs {
        let (r, m, s) = if knob == "cooldowns_ms" {
            (
                "cooldowns_ms = [7]".to_string(),
                "cooldowns_ms = [8]".to_string(),
                "cooldowns_ms = [9]".to_string(),
            )
        } else {
            (format!("{knob} = 7"), format!("{knob} = 8"), format!("{knob} = 9"))
        };
        let want = |n: &str| {
            if knob == "cooldowns_ms" { format!("[{n}]") } else { n.to_string() }
        };
        let mut row = Vec::new();
        for bits in 0 .. 8u8 {
            let (ur, um, us) = (bits & 4 != 0, bits & 2 != 0, bits & 1 != 0);
            let root_extra = if ur { format!("[timing]\n{r}\n") } else { String::new() };
            let member_extra = if um { format!("[timing]\n{m}\n") } else { String::new() };
            let inner_extra = if us {
                match form {
                    Form::Composed => format!("[sweep.s.timing]\n{s}\n"),
                    Form::Sections => format!("[bench.s.timing]\n{s}\n"),
                }
            } else {
                String::new()
            };
            let tag = format!("t-{label}-{knob}-{bits}");
            let v = match resolve(form, &tag, &root_extra, &member_extra, &inner_extra, "") {
                Ok(c) => read(&c),
                Err(e) => format!("ERR({e})"),
            };
            let lbl = format!(
                "{}{}{}",
                if ur { "R" } else { "-" },
                if um { "M" } else { "-" },
                if us { "S" } else { "-" }
            );
            row.push((lbl, v));
        }
        let get = |l: &str| row.iter().find(|(a, _)| a == l).unwrap().1.clone();
        println!(
            "  {:<14} {}",
            knob,
            row.iter()
                .map(|(a, b)| format!("{a}={b}"))
                .collect::<Vec<_>>()
                .join("  ")
        );
        // controls: each level must be able to move the value ALONE
        for (l, w, why) in [
            ("R--", want("7"), "C-root inert"),
            ("-M-", want("8"), "C-member inert"),
            ("--S", want("9"), "C-inner inert"),
        ] {
            if get(l) != w {
                failures.push(format!("{label}/timing.{knob}: {why}: {l}={}, want {w}", get(l)));
            }
        }
        // the undeclared knobs must keep the level below, which is the
        // defect class: declaring ONE knob must not move the other four
        for (l, w) in [("RM-", want("8")), ("R-S", want("9")), ("-MS", want("9")), ("RMS", want("9"))]
        {
            if get(l) != w {
                failures.push(format!("{label}/timing.{knob}: PRECEDENCE {l}={}, want {w}", get(l)));
            }
        }
    }
}

/// The defect class in its own right: declaring ONE knob at a level must
/// leave the other four inherited from the level below, not reset to the
/// framework default. This is what shipped broken on the sections form.
fn one_knob_isolation(form: Form, label: &str, failures: &mut Vec<String>) {
    println!();
    println!("== {label}: declaring ONE knob must not move the other four ==");
    // root declares all five to distinct non-default values; the member
    // declares only `passes`. The other four must stay at the root's.
    let root_extra = "[timing]\npasses = 7\nruns_per_pass = 77\nbatch_size = 777\n\
                      harness_runs = 7\ncooldowns_ms = [7]\n";
    let member_extra = "[timing]\npasses = 8\n";
    let tag = format!("iso-{label}");
    match resolve(form, &tag, root_extra, member_extra, "", "") {
        Ok(c) => {
            let got = format!(
                "passes={} runs_per_pass={} batch_size={} harness_runs={} cooldowns_ms={:?}",
                c.passes, c.runs_per_pass, c.batch_size, c.harness_runs, c.cooldowns_ms
            );
            let want = "passes=8 runs_per_pass=77 batch_size=777 harness_runs=7 cooldowns_ms=[7]";
            println!("  got : {got}");
            println!("  want: {want}");
            if got != want {
                failures.push(format!(
                    "{label}: one-knob isolation BROKEN. A member declaring only \
                     `passes` reset the other four to framework defaults."
                ));
            }
        },
        Err(e) => {
            println!("  ERR: {e}");
            failures.push(format!("{label}: one-knob isolation could not run: {e}"));
        },
    }
}

fn main() {
    let mut failures: Vec<String> = Vec::new();

    println!("== composed form: sweep against member, field by field ==");
    let fields: Vec<Field> = vec![
        Field {
            name: "title",
            base_lo: "title = \"D\"",
            lo: "title = \"M\"",
            hi: "title = \"S\"",
            default: "D",
            l: "M",
            h: "S",
            body: "arms = [\"packed\",\"dense\"]",
            read: |c| c.title.clone(),
        },
        Field {
            name: "workload",
            base_lo: "",
            lo: "workload = \"wm\"",
            hi: "workload = \"ws\"",
            default: "default",
            l: "wm",
            h: "ws",
            body: "title = \"base title\"\narms = [\"packed\",\"dense\"]",
            read: |c| c.workload.clone(),
        },
        Field {
            name: "master_seed",
            base_lo: "",
            lo: "master_seed = 11",
            hi: "master_seed = 22",
            default: "0",
            l: "11",
            h: "22",
            body: "title = \"base title\"\narms = [\"packed\",\"dense\"]",
            read: |c| c.master_seed.to_string(),
        },
        // booleans: C1 needs three distinct values and a bool has two, so
        // the higher level declares `false` against the lower's `true`.
        Field {
            name: "may_differ",
            base_lo: "",
            lo: "may_differ = true",
            hi: "may_differ = false",
            default: "false",
            l: "true",
            h: "false",
            body: "title = \"base title\"\narms = [\"packed\",\"dense\"]",
            read: |c| c.may_differ.to_string(),
        },
        Field {
            name: "required",
            base_lo: "",
            lo: "required = true",
            hi: "required = false",
            default: "false",
            l: "true",
            h: "false",
            body: "title = \"base title\"\narms = [\"packed\",\"dense\"]",
            read: |c| c.required.to_string(),
        },
        Field {
            name: "threaded",
            base_lo: "",
            lo: "threaded = true",
            hi: "threaded = false",
            default: "false",
            l: "true",
            h: "false",
            body: "title = \"base title\"\narms = [\"packed\",\"dense\"]",
            read: |c| c.threaded.to_string(),
        },
        Field {
            name: "arms",
            base_lo: "",
            lo: "arms = [\"packed\",\"dense\"]",
            hi: "arms = [\"dense\"]",
            default: "ERR",
            l: "2",
            h: "1",
            body: "title = \"base title\"",
            read: |c| c.variant_paths.len().to_string(),
        },
        Field {
            name: "baseline",
            base_lo: "",
            lo: "baseline = \"packed\"",
            hi: "baseline = \"dense\"",
            default: "none",
            l: "packed",
            h: "dense",
            body: "title = \"base title\"\narms = [\"packed\",\"dense\"]",
            read: |c| {
                c.normalise_baseline
                    .clone()
                    .unwrap_or_else(|| "none".into())
            },
        },
    ];
    for f in &fields {
        run_field(f, &mut failures);
    }

    timing_matrix(Form::Composed, "composed", &mut failures);
    timing_matrix(Form::Sections, "sections", &mut failures);
    one_knob_isolation(Form::Composed, "composed", &mut failures);
    one_knob_isolation(Form::Sections, "sections", &mut failures);

    println!();
    println!("(R = root bench.toml, M = member top level, S = sweep or section)");
    println!();
    if failures.is_empty() {
        println!("RESULT: every case as documented; C1/C2/C3 passed for every field and knob.");
    } else {
        println!("RESULT: {} failure(s):", failures.len());
        for f in &failures {
            println!("  {f}");
        }
        std::process::exit(1);
    }
}
