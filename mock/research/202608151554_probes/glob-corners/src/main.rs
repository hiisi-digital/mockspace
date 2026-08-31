//! Probe: the corners of `tree::glob_match` that its own suite does not
//! name.
//!
//! `glob_match` (`bench-harness/src/tree.rs:276-303`) decides benchspace
//! membership, and `["**"]` is the settled default, so it runs for every
//! consumer that adopts the form. Its suite
//! (`tree.rs:1031-1047`, 17 assertions) covers `**` at the root, `*`
//! not crossing `/`, and prefix / suffix / infix components. It does not
//! name: `**` matching ZERO components, `**` in the middle of a pattern,
//! `**` as a prefix, several `*` in one component, `*` matching the empty
//! string, or the empty pattern.
//!
//! The doc comment states the zero case explicitly: "A whole component of
//! `**` matches any number of components including zero." That is the one
//! this probe cares about most, because it is written down and unasserted.
//!
//! NEGATIVE CONTROLS, stated before the run:
//!   C1 a pair the existing suite asserts TRUE must read true here.
//!   C2 a pair the existing suite asserts FALSE must read false here.
//!   Together they show the instrument can produce both answers; a probe
//!   that only ever prints `true` is measuring nothing.

use mockspace_bench_harness::tree::glob_match;

fn row(pat: &str, path: &str, expected: Option<bool>) {
    let got = glob_match(pat, path);
    let verdict = match expected {
        None => "        ".to_string(),
        Some(e) if e == got => "  as documented".to_string(),
        Some(e) => format!("  DOCUMENTED {e}, GOT {got}"),
    };
    println!("  glob_match({pat:>12?}, {path:>14?}) = {got:<5}{verdict}");
}

fn main() {
    println!("controls (both from the existing suite, tree.rs:1031-1047):");
    let c1 = glob_match("a/**", "a/b/c");
    let c2 = glob_match("bench-*", "stranger");
    println!("  C1 glob_match(\"a/**\", \"a/b/c\") == true  : {}", if c1 { "PASS" } else { "FAIL" });
    println!("  C2 glob_match(\"bench-*\", \"stranger\") == false: {}", if !c2 { "PASS" } else { "FAIL" });
    if !c1 || c2 {
        println!("\ncontrols failed; findings void.");
        std::process::exit(1);
    }

    println!("\n`**` matching zero components (the doc comment says it does):");
    row("a/**", "a", Some(true));
    row("**", "", Some(true));
    row("a/**/b", "a/b", Some(true));

    println!("\n`**` as a prefix and in the middle:");
    row("**/b", "b", None);
    row("**/b", "a/b", None);
    row("**/b", "a/x/b", None);
    row("a/**/c", "a/b/c", None);
    row("a/**/c", "a/b/x/c", None);
    row("a/**/c", "a/c/c", None);

    println!("\nseveral `*` in one component, and `*` matching empty:");
    row("a*b*c", "abc", None);
    row("a*b*c", "aXbYc", None);
    row("a*", "a", None);
    row("*", "", None);

    println!("\nthe empty pattern:");
    row("", "", None);
    row("", "a", None);
    row("/", "a", None);

    println!("\ngrowth in the number of `*` in one component.");
    println!("This is an AD-HOC SPIKE, not a benchmark: it shows a shape,");
    println!("it prices nothing, and no fork may be decided on it.");
    println!("  stars  text  elapsed");
    let mut last: Option<f64> = None;
    for stars in 4 ..= 13 {
        let pat = format!("{}b", "a*".repeat(stars));
        let text = "a".repeat(24);
        let t = std::time::Instant::now();
        let r = glob_match(&pat, &text);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let ratio = last.map(|l| format!("  x{:.1} over the previous", ms / l)).unwrap_or_default();
        println!("  {stars:>5}  {:>4}  {ms:>8.1}ms  (match={r}){ratio}", text.len());
        last = Some(ms.max(0.001));
        if ms > 4000.0 {
            println!("  stopping: one more doubling is over eight seconds.");
            break;
        }
    }
    println!("\nThe matcher at tree.rs:277-289 backtracks without memoisation.");
    println!("The cost is COMBINATORIAL in (stars k, component length n), not");
    println!("simply exponential in k: the ratios above flatten past k = n/2");
    println!("because the number of ways to place k cut points in n characters");
    println!("peaks there and then falls. An earlier run of this probe with");
    println!("k = 16 against n = 40, which is much nearer that peak, had not");
    println!("returned after roughly 400 seconds and was killed. That is an");
    println!("existence claim about non-termination in practice; the table");
    println!("above is a shape, and neither is a price.");
    println!("Patterns are consumer-authored in `[benchspace] members` / `exclude`");
    println!("and are matched against every discovered path (tree.rs:197, :261),");
    println!("so this is a hazard rather than a live defect: the realistic");
    println!("patterns (`**`, `bench-*`, `*-probe`) sit far from the wall.");
}
