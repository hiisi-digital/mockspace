// A: is a const-array index legal in const-argument position, with no
// generic_const_exprs? If yes, "monomorphisation needs literal tokens" is
// false as stated: it needs a const EXPRESSION whose value is known, and an
// indexed const item qualifies.
struct Case<const KEY: usize>;
impl<const KEY: usize> Case<KEY> {
    const fn key() -> usize { KEY }
}

const POINTS: [usize; 3] = [80003, 130003, 160003];

fn main() {
    // const argument is a braced const expression indexing a const item.
    assert_eq!(Case::<{ POINTS[0] }>::key(), 80003);
    assert_eq!(Case::<{ POINTS[1] }>::key(), 130003);
    assert_eq!(Case::<{ POINTS[2] }>::key(), 160003);
    println!("A: PASS - Case<{{POINTS[i]}}> monomorphises from a const item, no literal needed");
}
