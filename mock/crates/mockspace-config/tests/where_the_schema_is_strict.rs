//! Where the schema refuses a key it does not have, and where it does not.
//!
//! The root of the file is strict, and so is the inline form of an import and a preset file. The
//! sections under the root are not: a misspelt key inside `[mockspace]` or `[hosts.<name>]` parses
//! and is dropped. Spec §46 says neither, so the design says what the code does and the question
//! of making the sections strict is open. The second test is the one that changes when it is
//! answered.

use mockspace_config::parse_mockspace_toml_str;

#[test]
fn a_key_the_root_does_not_have_is_refused() {
    let toml = "made_up_key = 1\n\n[mockspace]\nversion = \"1.0\"\n";
    let err = parse_mockspace_toml_str(toml).expect_err("an unknown root key parsed");
    assert!(
        format!("{err}").contains("made_up_key"),
        "the error names another key: {err}"
    );
}

#[test]
fn a_key_a_section_does_not_have_is_not_refused() {
    for toml in [
        "[mockspace]\nversion = \"1.0\"\nverison = \"2.0\"\n",
        "[mockspace]\nversion = \"1.0\"\n\n[hosts.origin]\nurl = \"https://example.invalid/r.git\"\nurll = \"x\"\n",
        "[mockspace]\nversion = \"1.0\"\n\n[refs]\nmade_up = true\n",
    ] {
        assert!(
            parse_mockspace_toml_str(toml).is_ok(),
            "a section refused an unknown key, so the design's account of where the schema is \
             strict is out of date:\n{toml}"
        );
    }
}
