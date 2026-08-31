// Probe: is 0 a fixed point of the bench harness bootstrap RNG, and what does
// that do to the confidence interval the report's Highlights section prints?
//
// Copy of `bench-harness/src/analysis.rs::bootstrap_mix` and
// `bootstrap_ci_median` as they stand on dev (commit 50c6e13), unmodified.
// Run: rustc -O bootstrap_seed_zero.rs -o /tmp/probe && /tmp/probe

fn bootstrap_mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D049BB133111EB);
    x ^= x >> 31;
    x
}

const BOOTSTRAP_ITERATIONS: usize = 10_000;
const CI_LOWER: f64 = 0.025;
const CI_UPPER: f64 = 0.975;

fn bootstrap_ci_median(vals: &[f64], seed: u64) -> (f64, f64, f64) {
    if vals.len() < 3 {
        let m = if vals.is_empty() { 0.0 } else { vals[0] };
        return (m, m, m);
    }
    let n = vals.len();
    let mut sorted = vals.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let true_median =
        if n % 2 == 0 { (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0 } else { sorted[n / 2] };
    let mut boot_medians = Vec::with_capacity(BOOTSTRAP_ITERATIONS);
    let mut rng = seed;
    for _ in 0 .. BOOTSTRAP_ITERATIONS {
        let mut resample = Vec::with_capacity(n);
        for _ in 0 .. n {
            rng = bootstrap_mix(rng);
            let idx = (rng as usize) % n;
            resample.push(sorted[idx]);
        }
        resample.sort_by(|a, b| a.total_cmp(b));
        let boot_med =
            if n % 2 == 0 { (resample[n / 2 - 1] + resample[n / 2]) / 2.0 } else { resample[n / 2] };
        boot_medians.push(boot_med);
    }
    boot_medians.sort_by(|a, b| a.total_cmp(b));
    let lo_idx = (BOOTSTRAP_ITERATIONS as f64 * CI_LOWER) as usize;
    let hi_idx = (BOOTSTRAP_ITERATIONS as f64 * CI_UPPER) as usize;
    (boot_medians[lo_idx], true_median, boot_medians[hi_idx.min(boot_medians.len() - 1)])
}

fn main() {
    println!("bootstrap_mix(0)          = {}", bootstrap_mix(0));
    println!("bootstrap_mix(mix(0))     = {}", bootstrap_mix(bootstrap_mix(0)));
    println!("bootstrap_mix(1)          = {}", bootstrap_mix(1));

    // Paired differences symmetric about zero: no honest test calls this
    // significant. min = -3.0, max = +3.0, median = 0.0.
    let diffs = vec![-3.0f64, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0];

    let (lo0, med0, hi0) = bootstrap_ci_median(&diffs, 0);
    println!("\nseed 0    : ci=[{lo0}, {hi0}] median={med0}");
    println!("  significant (ci_lo > 0 || ci_hi < 0) = {}", lo0 > 0.0 || hi0 < 0.0);

    let (lo1, med1, hi1) = bootstrap_ci_median(&diffs, 0xC0C0_CAFE);
    println!("seed nonzero: ci=[{lo1}, {hi1}] median={med1}");
    println!("  significant (ci_lo > 0 || ci_hi < 0) = {}", lo1 > 0.0 || hi1 < 0.0);

    // Control: with a nonzero seed the resample must not be constant.
    let mut rng = 0xC0C0_CAFEu64;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0 .. 8 {
        rng = bootstrap_mix(rng);
        seen.insert(rng % 7);
    }
    println!("\ncontrol: distinct indices drawn from nonzero seed = {}", seen.len());
    let mut rng0 = 0u64;
    let mut seen0 = std::collections::BTreeSet::new();
    for _ in 0 .. 8 {
        rng0 = bootstrap_mix(rng0);
        seen0.insert(rng0 % 7);
    }
    println!("control: distinct indices drawn from seed 0       = {}", seen0.len());

    // Two-sample case: is the returned "median" the smaller, the larger, or
    // just whichever came first?
    println!("\nbootstrap_ci_median(&[9.0, 1.0], 42) = {:?}", bootstrap_ci_median(&[9.0, 1.0], 42));
    println!("bootstrap_ci_median(&[1.0, 9.0], 42) = {:?}", bootstrap_ci_median(&[1.0, 9.0], 42));
}
