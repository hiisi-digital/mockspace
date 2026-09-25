//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------
//! Reading a seed off the command line, where a seed that does not parse is
//! refused rather than dropped.
//!
//! Both flags used to take what parsed and forget the rest. `--seed 0xZZ` ran
//! on the default seed, and a worker handed `--seeds` it could not read
//! validated no seed at all, printed nothing, and exited clean, which the
//! orchestrator reads the same as a variant that passed.

/// One seed, decimal or `0x` hex, with `_` allowed as a separator. The run
/// prints its master seed in hex for replay, so hex is what a person copies.
pub(super) fn parse_seed(s: &str) -> Result<u64, String> {
    let t = s.replace('_', "");
    let parsed = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        t.parse::<u64>()
    };
    parsed.map_err(|e| format!("`{s}` is not a seed, decimal or 0x hex: {e}"))
}

/// A comma-separated list of seeds, every one of which has to parse. `split`
/// yields at least one token, so an empty list is an empty seed and refused
/// as one.
pub(super) fn parse_seeds(s: &str) -> Result<Vec<u64>, String> {
    s.split(',').map(parse_seed).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decimal_seed_parses() {
        assert_eq!(parse_seed("42"), Ok(42));
    }

    #[test]
    fn a_hex_seed_parses_in_either_case_of_prefix() {
        assert_eq!(parse_seed("0x18d68e5e17921413"), Ok(0x18d6_8e5e_1792_1413));
        assert_eq!(parse_seed("0X2a"), Ok(42));
    }

    #[test]
    fn an_underscore_is_a_separator() {
        assert_eq!(parse_seed("1_000"), Ok(1000));
        assert_eq!(parse_seed("0xdead_beef"), Ok(0xdead_beef));
    }

    #[test]
    fn the_largest_seed_parses_and_one_past_it_is_refused() {
        assert_eq!(parse_seed("18446744073709551615"), Ok(u64::MAX));
        assert!(parse_seed("18446744073709551616").is_err());
        assert!(parse_seed("0x10000000000000000").is_err());
    }

    #[test]
    fn a_word_is_refused_and_named() {
        let e = parse_seed("banana").unwrap_err();
        assert!(e.contains("`banana`"), "{e}");
    }

    #[test]
    fn an_empty_seed_is_refused() {
        assert!(parse_seed("").is_err());
        assert!(parse_seed("0x").is_err());
    }

    #[test]
    fn a_negative_seed_is_refused() {
        assert!(parse_seed("-1").is_err());
    }

    #[test]
    fn a_list_of_seeds_parses_in_order() {
        assert_eq!(parse_seeds("3,0x1,2"), Ok(vec![3, 1, 2]));
    }

    #[test]
    fn a_hex_seed_in_a_list_is_read_rather_than_dropped() {
        // The case that validated nothing: one hex seed, which the old parser
        // could not read and silently left out.
        assert_eq!(parse_seeds("0x18d68e5e17921413"), Ok(vec![0x18d6_8e5e_1792_1413]));
    }

    #[test]
    fn one_bad_seed_refuses_the_whole_list() {
        let e = parse_seeds("1,x,3").unwrap_err();
        assert!(e.contains("`x`"), "{e}");
    }

    fn cli(args: &[&str]) -> Result<super::super::Cli, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        super::super::parse_cli(&args)
    }

    #[test]
    fn a_bad_seed_flag_refuses_the_run() {
        // It used to run on the default seed, so a typo in a replay produced a
        // different run that looked like the one asked for.
        let e = cli(&["driver", "--seed", "0xZZ"]).map(|_| ()).unwrap_err();
        assert!(e.contains("`0xZZ`"), "{e}");
    }

    #[test]
    fn a_seed_flag_with_no_value_refuses_the_run() {
        assert!(cli(&["driver", "--seed"]).is_err());
    }

    #[test]
    fn a_good_seed_flag_is_the_override() {
        assert_eq!(cli(&["driver", "--seed", "0x2a"]).map(|c| c.seed_override), Ok(Some(42)));
        assert_eq!(cli(&["driver"]).map(|c| c.seed_override), Ok(None));
    }

    #[test]
    fn an_empty_list_is_refused() {
        // `""` splits to one empty token, which is refused as a seed, and a
        // trailing comma leaves an empty token the same way.
        assert!(parse_seeds("").is_err());
        assert!(parse_seeds("1,").is_err());
    }
}
