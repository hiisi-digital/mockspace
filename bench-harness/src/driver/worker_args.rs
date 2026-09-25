//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------
//! The arguments the orchestrator hands a worker, read strictly.
//!
//! A flag that is absent takes its default. A flag that is present and does
//! not parse refuses the worker, since each of them used to fall back to its
//! default instead: `--seed 0x2a` ran on seed 0, and a validate worker handed
//! `--seeds` it could not read checked nothing and exited clean, which the
//! orchestrator reads the same as a variant that passed.

use super::seed::{parse_seed, parse_seeds};

/// What a worker was asked to do.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct WorkerArgs {
    pub dylib_path:  String,
    pub bench_name:  String,
    pub seed:        u64,
    pub cooldown_ms: u64,
    pub mode:        String,
    pub runs:        usize,
    pub batch:       usize,
    pub n:           usize,
    pub batch_k:     usize,
    pub max_call_us: Option<u64>,
    pub threaded:    bool,
    /// The seeds a validate worker checks, and nothing in any other mode.
    pub seeds:       Option<Vec<u64>>,
}

/// Reads `args`, refusing the first flag present with a value that does not
/// parse, a flag given with no value after it, and a validate worker with no
/// seeds.
pub(super) fn worker_args(args: &[String]) -> Result<WorkerArgs, String> {
    let get = |flag: &str| -> Result<Option<&str>, String> {
        match args.iter().position(|a| a == flag) {
            None => Ok(None),
            Some(pos) => match args.get(pos + 1) {
                Some(v) => Ok(Some(v.as_str())),
                None => Err(format!("`{flag}` wants a value after it")),
            },
        }
    };
    fn number<T: std::str::FromStr>(flag: &str, v: Option<&str>, default: T) -> Result<T, String>
    where
        T::Err: std::fmt::Display,
    {
        match v {
            None => Ok(default),
            Some(s) => s.parse().map_err(|e| format!("`{flag} {s}` is not a number: {e}")),
        }
    }

    let dylib_path = get("--worker")?.ok_or("--worker requires a dylib path")?.to_string();
    let seed = match get("--seed")? {
        None => 0,
        Some(s) => parse_seed(s).map_err(|e| format!("`--seed`: {e}"))?,
    };
    let mode = get("--mode")?.unwrap_or("warm").to_string();
    let seeds = if mode == "validate" {
        match get("--seeds")? {
            Some(s) => Some(parse_seeds(s).map_err(|e| format!("`--seeds`: {e}"))?),
            None => return Err("a validate worker wants `--seeds`, and without one it checks nothing".into()),
        }
    } else {
        None
    };
    Ok(WorkerArgs {
        dylib_path,
        bench_name: get("--bench-name")?.unwrap_or_default().to_string(),
        seed,
        cooldown_ms: number("--cooldown", get("--cooldown")?, 0)?,
        mode,
        runs: number("--runs", get("--runs")?, 0)?,
        batch: number("--batch", get("--batch")?, 1)?,
        n: number("--n", get("--n")?, 64)?,
        batch_k: number("--batch-k", get("--batch-k")?, 1)?,
        max_call_us: number("--max-call-us", get("--max-call-us")?, 0u64).map(|v| (v != 0).then_some(v))?,
        threaded: args.iter().any(|a| a == "--threaded"),
        seeds,
    })
}

#[cfg(test)]
mod tests;
