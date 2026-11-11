//! The segments of a `::`-joined identifier, as the reference syntax splits them.
//!
//! A task identifier and a namespace are paths of slugs, and a slug is written with hyphens
//! (`lower-pass`), which the syntax spells with an underscore. The syntax is the parser's, so the
//! split is its own: the text is read as a path with the hyphens folded, and each step is handed back
//! as the text it was written as.

use letkajenkka::Query;

/// Why a text is no path of segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentError {
    /// A `::` with nothing on one side of it, the byte offset in the text being where the step was
    /// meant to stand.
    Empty {
        position: usize,
    },
    /// The syntax refuses the text for another reason, which the error names.
    Syntax(letkajenkka::Error),
    /// A text that parses and is no plain path of names (a set, a wildcard, a line, a constraint).
    NotPlain,
}

/// The segments of `input`, each as written.
///
/// # Errors
///
/// [`SegmentError`] when the text is no plain path.
pub fn split(input: &str) -> Result<Vec<&str>, SegmentError> {
    let folded = input.replace('-', "_");
    let query = Query::parse(&folded).map_err(|e| {
        match e.kind {
            letkajenkka::Kind::EmptyStep => SegmentError::Empty { position: e.at },
            _ => SegmentError::Syntax(e),
        }
    })?;
    let names = query.path().and_then(letkajenkka::Path::plain).ok_or(SegmentError::NotPlain)?;
    // A step is a slice of the folded copy, which has the length of the original, so its place in the
    // copy is its place in the text.
    Ok(names
        .map(|n| {
            let at = n.as_ptr() as usize - folded.as_ptr() as usize;
            &input[at .. at + n.len()]
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mutation: splitting on `::` by hand accepts every row of the refusal tests below.
    #[test]
    fn a_path_is_split_into_the_segments_as_written() {
        assert_eq!(split("compiler::ir::lower-pass").unwrap(), ["compiler", "ir", "lower-pass"]);
        assert_eq!(split("workspace").unwrap(), ["workspace"]);
        assert_eq!(split("a-b-c::d").unwrap(), ["a-b-c", "d"]);
    }

    #[test]
    fn what_the_syntax_refuses_is_refused_and_named() {
        for (text, empty_at) in [("::foo", Some(0)), ("foo::", Some(5)), ("foo::::bar", Some(5))] {
            assert!(matches!(split(text), Err(SegmentError::Empty { position }) if Some(position) == empty_at), "{text}");
        }
        for text in ["compiler::IR::lower", "a :: b", "a::b c", "a::b.c", "-lead", "9lives", "a::2d"] {
            assert!(matches!(split(text), Err(SegmentError::Syntax(_))), "{text}: {:?}", split(text));
        }
        for text in ["a::{b, c}", "a::*", "a::3", "a::b<x>"] {
            assert_eq!(split(text), Err(SegmentError::NotPlain), "{text}");
        }
    }
}
