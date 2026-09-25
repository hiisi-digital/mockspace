//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

fn read(args: &[&str]) -> Result<WorkerArgs, String> {
    let mut all = vec!["driver", "--worker", "arm.so"];
    all.extend_from_slice(args);
    worker_args(&all.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn nothing_but_the_dylib_takes_every_default() {
    assert_eq!(
        read(&[]),
        Ok(WorkerArgs {
            dylib_path:  "arm.so".into(),
            bench_name:  String::new(),
            seed:        0,
            cooldown_ms: 0,
            mode:        "warm".into(),
            runs:        0,
            batch:       1,
            n:           64,
            batch_k:     1,
            max_call_us: None,
            threaded:    false,
            seeds:       None,
        })
    );
}

#[test]
fn every_flag_is_read_when_it_parses() {
    let a = read(&[
        "--bench-name", "b", "--seed", "7", "--cooldown", "5", "--mode", "cold", "--runs", "3",
        "--batch", "2", "--n", "16", "--batch-k", "4", "--max-call-us", "9", "--threaded",
    ])
    .unwrap();
    assert_eq!(
        (a.bench_name.as_str(), a.seed, a.cooldown_ms, a.mode.as_str(), a.runs, a.batch),
        ("b", 7, 5, "cold", 3, 2)
    );
    assert_eq!((a.n, a.batch_k, a.max_call_us, a.threaded), (16, 4, Some(9), true));
}

#[test]
fn a_hex_seed_is_the_seed_rather_than_zero() {
    // The replay case: the run prints its seed in hex, a person pastes it, and
    // the worker used to parse decimal only and run on seed 0.
    assert_eq!(read(&["--seed", "0x18d68e5e17921413"]).map(|a| a.seed), Ok(0x18d6_8e5e_1792_1413));
}

#[test]
fn a_seed_that_does_not_parse_refuses_the_worker() {
    let e = read(&["--seed", "0xZZ"]).unwrap_err();
    assert!(e.contains("--seed") && e.contains("`0xZZ`"), "{e}");
}

#[test]
fn every_number_that_does_not_parse_refuses_the_worker_and_names_its_flag() {
    for flag in ["--cooldown", "--runs", "--batch", "--n", "--batch-k", "--max-call-us"] {
        let e = read(&[flag, "lots"]).unwrap_err();
        assert!(e.contains(flag) && e.contains("lots"), "{flag}: {e}");
    }
}

#[test]
fn a_flag_with_nothing_after_it_refuses_the_worker() {
    for flag in ["--seed", "--n", "--mode", "--bench-name"] {
        assert!(read(&[flag]).is_err(), "{flag} with no value was read");
    }
}

#[test]
fn no_dylib_refuses_the_worker() {
    let args: Vec<String> = ["driver", "--n", "4"].iter().map(|s| s.to_string()).collect();
    assert!(worker_args(&args).is_err());
}

#[test]
fn a_zero_call_bound_is_no_bound() {
    assert_eq!(read(&["--max-call-us", "0"]).map(|a| a.max_call_us), Ok(None));
}

#[test]
fn a_validate_worker_with_no_seeds_is_refused() {
    // The failure the flag's check is for: a worker that checks nothing and
    // exits clean reads to the orchestrator as a variant that passed.
    let e = read(&["--mode", "validate"]).unwrap_err();
    assert!(e.contains("--seeds"), "{e}");
}

#[test]
fn a_validate_worker_with_a_bad_seed_is_refused() {
    let e = read(&["--mode", "validate", "--seeds", "1,x"]).unwrap_err();
    assert!(e.contains("--seeds") && e.contains("`x`"), "{e}");
}

#[test]
fn a_validate_worker_reads_every_seed_it_was_given() {
    assert_eq!(
        read(&["--mode", "validate", "--seeds", "3,0x1,2"]).map(|a| a.seeds),
        Ok(Some(vec![3, 1, 2]))
    );
}

#[test]
fn seeds_outside_validate_are_not_read() {
    // Only a validate worker checks seeds, so a list on another mode is left
    // alone rather than refused.
    assert_eq!(read(&["--seeds", "junk"]).map(|a| a.seeds), Ok(None));
}
