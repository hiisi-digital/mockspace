// B: can macro_rules! iterate a const array to emit one arm per element?
// Expected: no. A macro sees tokens; POINTS is an item whose value exists only
// after const evaluation, which happens long after expansion.
const POINTS: [usize; 3] = [64, 256, 1024];

macro_rules! arm_per_element {
    ($arr:ident) => {
        // there is no repetition operator that can walk $arr's VALUES.
        // The nearest legal thing walks a token list, not a const item.
        $( const _X: usize = $arr[0]; )*
    };
}

fn main() {
    arm_per_element!(POINTS);
}
