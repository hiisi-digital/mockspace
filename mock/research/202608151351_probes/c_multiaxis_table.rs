// C: a multi-axis dispatch table where the AXIS VALUES live in one const table
// and only the ARITY is tokens. This is the shape that would let bench.toml be
// the single source of a point set with named axes.
//
// Negative control at the bottom: two distinct points must reach distinct
// monomorphisations, so the table cannot be silently collapsing.

#[derive(Clone, Copy)]
struct Point { w: usize, nc: usize, op: usize, d: usize }

// generated from bench.toml; the ONLY place an axis value is written.
const PTS: [Point; 3] = [
    Point { w: 13, nc: 0, op: 0, d: 3 },
    Point { w: 64, nc: 1, op: 0, d: 3 },
    Point { w: 13, nc: 0, op: 1, d: 8 },
];

// the consumer's measured op: four const params, not one.
fn run<const W: usize, const NC: usize, const OP: usize, const D: usize>() -> usize {
    W * 1_000_000 + NC * 100_000 + OP * 10_000 + D
}

// the generated part is a bare index list. No axis value appears here.
macro_rules! dispatch {
    ( $( $i:literal ),* ) => {
        fn dispatch(idx: usize) -> Option<usize> {
            match idx {
                $( $i => Some(run::<{PTS[$i].w}, {PTS[$i].nc}, {PTS[$i].op}, {PTS[$i].d}>()), )*
                _ => None,
            }
        }
        // a const fn-pointer table over the same instantiations, to show the
        // shape also works without a match.
        const TABLE: [fn() -> usize; 3] = [
            $( run::<{PTS[$i].w}, {PTS[$i].nc}, {PTS[$i].op}, {PTS[$i].d}>, )*
        ];
    };
}
dispatch!(0, 1, 2);

fn main() {
    assert_eq!(dispatch(0), Some(13_000_003));
    assert_eq!(dispatch(1), Some(64_100_003));
    assert_eq!(dispatch(2), Some(13_010_008));
    assert_eq!(dispatch(3), None);
    for i in 0..3 { assert_eq!(TABLE[i](), dispatch(i).unwrap()); }
    // negative control: distinct points must be distinct monomorphisations.
    assert_ne!(dispatch(0), dispatch(2), "control: points 0 and 2 differ only in OP and D");
    assert_ne!(TABLE[0] as usize, TABLE[1] as usize, "control: distinct fn items");
    println!("C: PASS - 4 axes, values only in PTS, only the index list is generated tokens");
}
