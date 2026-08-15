// E: keep ONE const generic parameter and make it carry every axis, using
// adt_const_params (ALLOWED per unstable-features.md, #95174) rather than
// lifting the single-const-generic restriction.
#![feature(adt_const_params)]
#![feature(const_param_ty_trait)]
#![allow(incomplete_features)]

use std::marker::ConstParamTy;

#[derive(PartialEq, Eq, ConstParamTy, Clone, Copy, Debug)]
struct Point { w: usize, nc: usize, op: usize, d: usize }

const PTS: [Point; 3] = [
    Point { w: 13, nc: 0, op: 0, d: 3 },
    Point { w: 64, nc: 1, op: 0, d: 3 },
    Point { w: 13, nc: 0, op: 1, d: 8 },
];

// ONE const parameter, four axes. The measured fn reads named fields, not
// digits of a packed integer.
fn run<const P: Point>() -> usize {
    P.w * 1_000_000 + P.nc * 100_000 + P.op * 10_000 + P.d
}

macro_rules! dispatch {
    ( $( $i:literal ),* ) => {
        fn dispatch(idx: usize) -> Option<usize> {
            match idx { $( $i => Some(run::<{PTS[$i]}>()), )* _ => None }
        }
    };
}
dispatch!(0, 1, 2);

fn main() {
    assert_eq!(dispatch(0), Some(13_000_003));
    assert_eq!(dispatch(1), Some(64_100_003));
    assert_eq!(dispatch(2), Some(13_010_008));
    assert_eq!(dispatch(3), None);
    assert_ne!(dispatch(0), dispatch(2), "control: distinct points, distinct results");
    println!("E: PASS - one const param of struct type carries four named axes");
}
