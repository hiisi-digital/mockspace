//! The lines that name an instance's files, in the exclude file and in the sparse-checkout file.

use mockspace_store::{Held, exclude_lines, sparse_lines};

// Mutation: leave the directory line out of a pattern's lines; an excluded directory is never looked
// into, so a file below `notes/` that the pattern `notes/*` does not name would stay hidden.
#[test]
fn a_pattern_is_followed_by_the_line_that_takes_directories_back_out() {
    let held = Held::new(&["notes/*"]).unwrap();
    assert_eq!(exclude_lines("mock/research", Some(&held)), ["/mock/research/notes/*", "!/mock/research/notes/*/"]);
}

// Mutation: write the sparse lines by another route than the exclude lines, as the first version did.
#[test]
fn the_sparse_lines_are_the_exclude_lines_with_each_negation_toggled() {
    let held = Held::new(&["notes/*", "**/*.md.tmpl"]).unwrap();
    for held in [None, Some(&held)] {
        let (e, s) = (exclude_lines("mock/crates", held), sparse_lines("mock/crates", held));
        assert_eq!(e.len(), s.len());
        for (e, s) in e.iter().zip(&s) {
            assert_eq!(e.strip_prefix('!').is_some(), s.strip_prefix('!').is_none(), "{e} and {s}");
            assert_eq!(e.trim_start_matches('!'), s.trim_start_matches('!'));
        }
    }
    assert_eq!(sparse_lines("mock/research", None), ["!/mock/research/"]);
}

// Mutation: leave the directory unescaped, so `[1]` is a class that matches `1` and a space or `#`
// ends or starts a pattern.
#[test]
fn a_directory_is_escaped_for_what_a_pattern_reads_as_syntax() {
    assert_eq!(exclude_lines("mock/odd[1]", None), ["/mock/odd\\[1\\]/"]);
    assert_eq!(exclude_lines("a b/#c/!d", None), ["/a\\ b/\\#c/\\!d/"]);
    assert_eq!(exclude_lines("a*b?c", None), ["/a\\*b\\?c/"]);
    assert_eq!(exclude_lines("", None), ["/"]);
}
