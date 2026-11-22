//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A string written into a TOML document, quoted and escaped.
//!
//! Every path the tool writes into a manifest goes through [`basic_string`],
//! so a Windows path's backslashes reach the parser as backslashes rather
//! than as escapes (`C:\Users` would otherwise read as an invalid `\U`).

/// `s` as a TOML basic string, quotes included: `\` and `"` escaped, and every
/// control character written as `\uXXXX`. Valid where a TOML value is, also
/// inside a literal string that is parsed as TOML again
/// (`mockspace = '{ path = "C:\\x" }'`), since a literal string keeps the
/// escapes for that second parse.
pub fn basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::basic_string;

    #[derive(serde::Deserialize)]
    struct Dep {
        path: String,
    }

    #[test]
    fn a_windows_path_reads_back_as_itself() {
        for path in [
            r"C:\Users\runner\work\mockspace\lint-rules",
            r"\\server\share\a b\x",
            "/home/u/mockspace",
            "with \"quotes\" and\ttab",
            "unicode é",
        ] {
            let doc = format!("path = {}", basic_string(path));
            let dep: Dep = toml::from_str(&doc).unwrap_or_else(|e| panic!("{doc}: {e}"));
            assert_eq!(dep.path, path);
            // The nested shape a bench manifest uses: a literal string holding
            // an inline table, parsed a second time.
            let outer = format!("spec = '{{ path = {} }}'", basic_string(path));
            #[derive(serde::Deserialize)]
            struct Outer {
                spec: String,
            }
            if !path.contains('\'') {
                let o: Outer = toml::from_str(&outer).unwrap_or_else(|e| panic!("{outer}: {e}"));
                let inner: Dep = toml::from_str(&format!("x = {}", o.spec))
                    .map(|t: std::collections::BTreeMap<String, Dep>| {
                        t.into_values().next().unwrap()
                    })
                    .unwrap_or_else(|e| panic!("{}: {e}", o.spec));
                assert_eq!(inner.path, path);
            }
        }
    }

    #[test]
    fn an_unescaped_windows_path_does_not_read_back() {
        // The failure this exists for: the path written bare.
        let doc = format!("path = \"{}\"", r"C:\Users\x");
        assert!(toml::from_str::<Dep>(&doc).is_err());
    }
}
