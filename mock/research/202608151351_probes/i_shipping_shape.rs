// I: the whole shape, end to end, in the form an arm cdylib would take.
//   - the measured fn keeps ONE const generic parameter (so #[bench_variant]'s
//     existing rule is untouched); that parameter is a struct carrying every axis.
//   - the FFI entry takes one usize, and it is an INDEX into the generated point
//     table rather than a packed value.
//   - only the index list is generated tokens; every axis value lives in PTS.
#![feature(adt_const_params)]
use std::marker::ConstParamTy;

#[derive(PartialEq, Eq, ConstParamTy, Clone, Copy, Debug)]
pub struct Point { pub w: usize, pub nc: usize, pub op: usize, pub d: usize }

// generated from bench.toml. warm-container-width-l1 and -width-l2, the two
// sweeps that differ ONLY in the held nc, taken from arvo's real manifest.
pub const PTS: [Point; 4] = [
    Point { w: 8,  nc: 0, op: 0, d: 3 },   // width-l1, w=8
    Point { w: 13, nc: 0, op: 0, d: 3 },   // width-l1, w=13
    Point { w: 8,  nc: 1, op: 0, d: 3 },   // width-l2, w=8
    Point { w: 13, nc: 1, op: 0, d: 3 },   // width-l2, w=13
];

const N_SMALL: usize = 8_192;
const N_LARGE: usize = 1_048_576;

// the consumer's measured op: reads named axes, no digit extraction anywhere.
fn run<const P: Point>() -> u64 {
    let n = if P.nc == 0 { N_SMALL } else { N_LARGE };
    (P.w as u64) << 40 | (n as u64) << 8 | (P.d as u64)
}

macro_rules! entry {
    ( $( $i:literal ),* ) => {
        /// the FFI entry. `idx` indexes PTS; it is not a packed value.
        pub extern "C" fn bench_entry(idx: usize) -> u64 {
            match idx { $( $i => run::<{PTS[$i]}>(), )* _ => u64::MAX }
        }
    };
}
entry!(0, 1, 2, 3);

fn main() {
    // the two sweeps' w=8 rows are distinct measurements, distinguished by nc.
    assert_ne!(bench_entry(0), bench_entry(2),
        "control: width-l1 and width-l2 at w=8 must not collapse");
    assert_ne!(bench_entry(0), bench_entry(1),
        "control: distinct widths must not collapse");
    // out of range is refused, not silently served.
    assert_eq!(bench_entry(4), u64::MAX, "control: unknown index must refuse");
    assert_eq!(bench_entry(0) >> 40, 8);
    assert_eq!((bench_entry(2) >> 8) & 0xffff_ffff, N_LARGE as u64);

    // the hazard the design must close: under VALUE semantics these same four
    // rows are keyed 80003/130003/81003/131003, and under INDEX semantics
    // 0/1/2/3. A stale dylib built for one and called with the other is only
    // caught if the encoding is folded into the ABI hash.
    let value_keys = [80003usize, 130003, 81003, 131003];
    let index_keys = [0usize, 1, 2, 3];
    assert!(value_keys.iter().all(|v| !index_keys.contains(v)),
        "here they happen not to collide");
    // but vehje's own declared point list contains 1, 2, 4, 8, 16, 32
    // (vehje/mock/benches/src/main.rs:46-48), so for a sweep of >= 2 points the
    // index range and the value range DO intersect:
    let vehje_points = [1usize, 2, 4, 8, 16, 32, 64, 128, 256, 1024,
                        2048, 3072, 4096, 6144, 8192, 16384];
    let colliding: Vec<usize> =
        (0 .. vehje_points.len()).filter(|i| vehje_points.contains(i)).collect();
    assert!(!colliding.is_empty());
    println!("I: PASS - one struct const param, index-keyed FFI, controls hold");
    println!("I: index/value collision on vehje's own point list at indices {colliding:?}");
}
