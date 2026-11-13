//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a view writes down: the questions a unit of work put to the reading.
//!
//! A question names what was asked and nothing of the answer. The answer's digest is taken when
//! the trace is saved, from the same reading the lint was answered from, and taken again when the
//! trace is asked on a later run. Paths are written relative to the reading's root, so a trace
//! saved in one checkout is asked against another checkout's files and not the first one's.

use std::collections::BTreeSet;

/// One thing a lint asked for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Question {
    /// A file's content: its text, its bytes, its tree or its parsed TOML, which all follow from
    /// the same bytes.
    File(String),
    /// The names in a directory and whether each is a file or a directory.
    List(String),
    /// Whether a path is a file, a directory, or nothing.
    Kind(String),
    /// A read-only `git` invocation, by the directory it ran in and its arguments.
    Git { cwd: String, args: Vec<String> },
    /// Another command, by program, arguments and directory, answered for by the files it was said
    /// to depend on and the text it was given on its input. Never run again to check a trace.
    Run {
        program: String,
        args:    Vec<String>,
        cwd:     String,
        stdin:   Option<String>,
        inputs:  Vec<String>,
    },
    /// A value the engine hands over with a context, by its name.
    Value(String),
}

impl Question {
    /// The question as one line, which is how a trace is saved and how answers are shared.
    pub(crate) fn encode(&self) -> String {
        let mut out = String::new();
        let field = |s: &str, out: &mut String| {
            out.push('\u{1f}');
            escape_into(s, out);
        };
        match self {
            Self::File(p) => {
                out.push('F');
                field(p, &mut out);
            },
            Self::List(p) => {
                out.push('L');
                field(p, &mut out);
            },
            Self::Kind(p) => {
                out.push('K');
                field(p, &mut out);
            },
            Self::Value(k) => {
                out.push('V');
                field(k, &mut out);
            },
            Self::Git {
                cwd,
                args,
            } => {
                out.push('G');
                field(cwd, &mut out);
                for a in args {
                    field(a, &mut out);
                }
            },
            Self::Run {
                program,
                args,
                cwd,
                stdin,
                inputs,
            } => {
                out.push('R');
                field(program, &mut out);
                field(cwd, &mut out);
                field(stdin.as_deref().unwrap_or("-"), &mut out);
                field(&args.len().to_string(), &mut out);
                for a in args.iter().chain(inputs) {
                    field(a, &mut out);
                }
            },
        }
        out
    }

    /// The question [`Self::encode`] wrote, or `None` for a line in no shape it writes.
    pub(crate) fn decode(line: &str) -> Option<Self> {
        let mut parts = line.split('\u{1f}');
        let tag = parts.next()?;
        let mut fields = Vec::new();
        for p in parts {
            fields.push(unescape(p)?);
        }
        let one = |fields: Vec<String>| -> Option<String> {
            let [f] = <[String; 1]>::try_from(fields).ok()?;
            Some(f)
        };
        Some(match tag {
            "F" => Self::File(one(fields)?),
            "L" => Self::List(one(fields)?),
            "K" => Self::Kind(one(fields)?),
            "V" => Self::Value(one(fields)?),
            "G" => {
                let mut it = fields.into_iter();
                let cwd = it.next()?;
                Self::Git {
                    cwd,
                    args: it.collect(),
                }
            },
            "R" => {
                let mut it = fields.into_iter();
                let program = it.next()?;
                let cwd = it.next()?;
                let stdin = it.next()?;
                let n: usize = it.next()?.parse().ok()?;
                let rest: Vec<String> = it.collect();
                if rest.len() < n {
                    return None;
                }
                let (args, inputs) = rest.split_at(n);
                Self::Run {
                    program,
                    args: args.to_vec(),
                    cwd,
                    stdin: (stdin != "-").then_some(stdin),
                    inputs: inputs.to_vec(),
                }
            },
            _ => return None,
        })
    }
}

/// Everything one unit of work asked, or the reason it cannot be answered for.
///
/// A unit whose trace is untraced is run on every gate: it depends on something a trace cannot
/// hold, an environment variable or the clock, and said so through
/// [`View::untraced`](crate::reading::View::untraced).
#[derive(Debug, Clone, Default)]
pub struct Trace {
    pub(crate) questions: BTreeSet<Question>,
    pub(crate) untraced:  Option<String>,
}

impl Trace {
    /// How many distinct questions were asked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.questions.len()
    }

    /// Whether nothing was asked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.questions.is_empty()
    }

    /// Why this unit cannot be saved, when it said so.
    #[must_use]
    pub fn untraced(&self) -> Option<&str> {
        self.untraced.as_deref()
    }

    /// The questions, one line each, for a test or a report to read.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.questions.iter().map(Question::encode).collect()
    }

    pub(crate) fn absorb(&mut self, other: &Trace) {
        self.questions.extend(other.questions.iter().cloned());
        if self.untraced.is_none() {
            self.untraced.clone_from(&other.untraced);
        }
    }
}

/// Escape the characters a saved line gives a meaning to.
pub(crate) fn escape_into(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{1f}' => out.push_str("\\s"),
            c => out.push(c),
        }
    }
}

/// The text [`escape_into`] wrote, or `None` for an escape it never writes.
pub(crate) fn unescape(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match it.next()? {
            '\\' => '\\',
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            's' => '\u{1f}',
            _ => return None,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_question_reads_back_as_written() {
        let all = [
            Question::File("a/b.rs".into()),
            Question::List("a\tb\nc\\d\u{1f}e".into()),
            Question::Kind("/abs/x".into()),
            Question::Value("deps:acme-core".into()),
            Question::Git {
                cwd:  "mock".into(),
                args: vec!["diff".into(), "--cached".into(), String::new()],
            },
            Question::Git {
                cwd:  String::new(),
                args: Vec::new(),
            },
            Question::Run {
                program: "rustfmt".into(),
                args:    vec!["--emit=stdout".into()],
                cwd:     "mock".into(),
                stdin:   Some("0123".into()),
                inputs:  vec!["rustfmt.toml".into(), "x".into()],
            },
            Question::Run {
                program: "tool".into(),
                args:    Vec::new(),
                cwd:     String::new(),
                stdin:   None,
                inputs:  Vec::new(),
            },
        ];
        for q in all {
            let line = q.encode();
            assert!(!line.contains('\n') && !line.contains('\t'), "one line, no tab: {line:?}");
            assert_eq!(Question::decode(&line), Some(q));
        }
    }

    #[test]
    fn a_line_in_no_written_shape_is_refused() {
        for bad in ["", "Z\u{1f}x", "F", "F\u{1f}a\u{1f}b", "R\u{1f}p\u{1f}c\u{1f}-\u{1f}3\u{1f}a", "F\u{1f}\\q"] {
            assert_eq!(Question::decode(bad), None, "{bad:?}");
        }
    }
}
