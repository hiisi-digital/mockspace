use super::{COPY_AT, containment, from_bytes, sketch, to_bytes};

/// A text of `n` words drawn from a fixed vocabulary by a generator seeded with `seed`: the same
/// seed gives the same text, and two seeds share almost no fivegram.
fn passage(seed: u64, n: usize) -> String {
    let mut x = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    (0 .. n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            format!("w{}", x % 400)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_copy_is_contained_whole_and_an_unrelated_text_not_at_all() {
    let a = sketch(&passage(1, 600));
    let copy = sketch(&passage(1, 600));
    assert_eq!(containment(&a, &copy), 1.0);

    let other = sketch(&passage(2, 600));
    assert!(containment(&a, &other) < 0.1, "unrelated text folded in");
}

#[test]
fn an_edited_copy_still_folds_and_a_rewrite_does_not() {
    let original = passage(1, 600);
    // Another hundred words appended: the copy gains text the original lacks.
    let grown = format!("{original} {}", passage(3, 100));
    let c = containment(&sketch(&original), &sketch(&grown));
    assert!(c >= COPY_AT, "an original inside a longer copy is a copy: {c}");

    // Half of it replaced.
    let half = format!("{} {}", &original[.. original.len() / 2], passage(4, 300));
    let c = containment(&sketch(&original), &sketch(&half));
    assert!(c < COPY_AT, "half rewritten is not a copy: {c}");
}

#[test]
fn a_text_too_short_to_sample_is_never_a_copy_of_anything() {
    // THE CONTROL: a long text is a copy of itself.
    let short = sketch("one two three four five six");
    assert_eq!(containment(&short, &short), 0.0);
    let long = sketch(&passage(1, 600));
    assert_eq!(containment(&long, &long), 1.0);
}

#[test]
fn a_sketch_survives_the_bytes_it_is_stored_as() {
    let s = sketch(&passage(1, 400));
    assert!(!s.is_empty());
    assert_eq!(from_bytes(&to_bytes(&s)), s);
    // A truncated blob loses its last word and nothing else.
    let mut b = to_bytes(&s);
    b.truncate(b.len() - 3);
    assert_eq!(from_bytes(&b), s[.. s.len() - 1]);
}
