//! The lib tests' name for the one executable writer, from the file the
//! integration tests include too.

#[path = "../tests/common/exec.rs"]
mod exec;

pub(crate) use exec::write_executable;
