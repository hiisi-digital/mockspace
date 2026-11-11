//! A file the migration would rewrite whose bytes, or whose name, are not UTF-8. A rewrite of the
//! text read in place of those bytes writes other bytes than the file held, and a name read in
//! place of the real one names no file, so the template is never rewritten while the run says the
//! tree is migrated. Either is refused before anything is written, naming the file.

use std::os::unix::ffi::OsStrExt;

use super::*;

// Mutation: read the registry file lossily, as before; the run then writes U+FFFD over the byte.
#[test]
fn a_registry_file_holding_a_byte_that_is_not_utf8_is_refused_and_left_as_it_was() {
    let r = repo();
    let path = r.cfg.mock_dir.join("registry/rulings.toml");
    let mut bytes = RULINGS.as_bytes().to_vec();
    bytes.extend_from_slice(b"# caf\xe9, written in latin-1\n");
    std::fs::write(&path, &bytes).unwrap();
    let why = plan_of(&r.cfg, &[NAME]).unwrap_err().join("\n");
    assert!(why.contains("rulings.toml") && why.contains("UTF-8"), "{why}");
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::FAILURE);
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "the file keeps every byte");
    assert_eq!(read(&r.cfg, "docs/DOC.md.tmpl"), TEMPLATE, "nothing else is written either");
}

// Mutation: read the names git lists lossily, as before; the template is then left out without a
// word and the run says the tree is migrated.
#[test]
fn a_template_whose_name_is_not_utf8_is_refused_and_not_left_out() {
    let r = repo();
    let name = std::ffi::OsStr::from_bytes(b"docs/caf\xe9.md.tmpl");
    std::fs::write(r.cfg.repo_root.join(name), TEMPLATE).unwrap();
    let why = plan_of(&r.cfg, &[NAME]).unwrap_err().join("\n");
    assert!(why.contains("caf") && why.contains("UTF-8"), "{why}");
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::FAILURE);
    assert_eq!(std::fs::read(r.cfg.repo_root.join(name)).unwrap(), TEMPLATE.as_bytes());
    assert_eq!(read(&r.cfg, "docs/DOC.md.tmpl"), TEMPLATE, "nothing else is written either");
}
