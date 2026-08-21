#[unsafe(no_mangle)]
pub extern "C" fn run_c(x: u64) -> u64 { support::common(x) }
