#[unsafe(no_mangle)]
pub extern "C" fn arm_entry(x: u64) -> u64 { shared::common(x) }
#[unsafe(no_mangle)]

#[unsafe(no_mangle)]
pub extern "C" fn arm_probe() -> u64 { shared::common(1) }
