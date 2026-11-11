//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

const SAMPLE: &str = "# the file\n\nversion = 1\n\n# about a\n[[law]]\nid = \"a\"\ntext = \"\"\"\n[[fake]]\n\"\"\" # note\n\n[law.sub]\nx = 1\n\n# detached\n\n[[law]]\nid = \"b\"\n[meta]\nk = 2\n";

#[test]
fn a_file_is_cut_without_losing_a_byte() {
    for text in [
        SAMPLE,
        "",
        "x = 1\n",
        "[[a]]\nid = \"1\"",
        "# only a comment\n",
        "[[a]]\r\nid = \"1\"\r\n\r\n[[a]]\r\nid = \"2\"\r\n",
    ] {
        let doc = Doc::parse(text).unwrap();
        assert_eq!(doc.render(), text);
    }
}

#[test]
fn blocks_are_keyed_by_namespace_and_id_or_by_table_path() {
    let doc = Doc::parse(SAMPLE).unwrap();
    let keys: Vec<String> = doc.keys().iter().map(|k| k.to_string()).collect();
    assert_eq!(keys, ["[[law]] id = \"a\"", "[[law]] id = \"b\"", "[meta]"]);
}

#[test]
fn a_row_carries_its_comment_its_subtable_and_a_trailing_comment() {
    let doc = Doc::parse(SAMPLE).unwrap();
    let a = &doc.blocks[0];
    assert!(a.chunk.starts_with("# about a\n[[law]]"), "{:?}", a.chunk);
    assert!(a.chunk.contains("[law.sub]\nx = 1"), "{:?}", a.chunk);
    assert!(a.chunk.contains("\"\"\" # note\n"), "{:?}", a.chunk);
    // a string holding a header-looking line is not a block
    assert_eq!(doc.blocks.len(), 3);
}

#[test]
fn the_file_comment_and_scalars_stay_in_the_head_apart_from_the_row_comment() {
    let doc = Doc::parse(SAMPLE).unwrap();
    assert_eq!(doc.head, "# the file\n\nversion = 1\n\n");
}

#[test]
fn a_detached_comment_travels_with_the_row_above_it() {
    let doc = Doc::parse(SAMPLE).unwrap();
    assert!(
        doc.blocks[0].tail.contains("# detached"),
        "{:?}",
        doc.blocks[0].tail
    );
}

#[test]
fn a_file_the_merge_cannot_key_is_refused() {
    for (text, why) in [
        ("[[law]]\nid = \"a\"\n[[law]]\nid = \"a\"\n", "twice"),
        ("[[law]]\nname = \"a\"\n", "no string id"),
        ("[[law]]\nid = 3\n", "no string id"),
        ("[[law]\nid = \"a\"\n", "not valid TOML"),
    ] {
        let err = Doc::parse(text).unwrap_err();
        assert!(err.0.contains(why), "{text:?}: {}", err.0);
    }
}

#[test]
fn rows_of_one_namespace_may_be_separated_by_other_tables() {
    let text = "[[a]]\nid = \"1\"\n[t]\nk = 1\n[[a]]\nid = \"2\"\n";
    let doc = Doc::parse(text).unwrap();
    assert_eq!(doc.blocks.len(), 3);
    assert_eq!(doc.render(), text);
}
