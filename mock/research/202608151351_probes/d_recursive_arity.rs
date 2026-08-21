// D: can the ARITY be avoided too? A recursive const-generic trampoline would
// produce K monomorphisations from one number K, so nothing about the point set
// would be in Rust at all. It needs `I - 1` in const-argument position.
const PTS: [usize; 3] = [64, 256, 1024];

fn run<const N: usize>() -> usize { N }

fn walk<const I: usize>(acc: &mut Vec<usize>) {
    if I == 0 { return; }
    acc.push(run::<{ PTS[I - 1] }>());
    walk::<{ I - 1 }>(acc);
}

fn main() {
    let mut v = Vec::new();
    walk::<3>(&mut v);
    println!("{v:?}");
}
