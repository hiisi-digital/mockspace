//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What stops a run before anything reads the mock directory.

/// What stops a run before anything reads the mock directory: a declared ref with no instance here,
/// whose files would otherwise read as a directory with nothing in it, and a file a ref holds that
/// the branch still tracks, which is then in two places that can differ. Every one is named.
pub fn standing(cfg: &crate::config::Config) -> Result<(), Vec<String>> {
    let mut why = Vec::new();
    for d in &cfg.data_refs {
        let ns = d.namespace();
        let found = (|| -> Result<Option<String>, String> {
            let store = d.open(&cfg.repo_root)?;
            let inst = d.instance(&store, &cfg.mock_dir)?;
            if inst.base().map_err(|e| e.to_string())?.is_none() {
                return Ok(Some(format!(
                    "the data ref `{ns}` has no instance at {}, so `cargo mock data-refs instantiate {ns}` fetches it",
                    d.instance_dir(&cfg.mock_dir).display()
                )));
            }
            // a clean of the work tree takes the files and leaves the record, and the directory then
            // reads as one with nothing in it
            if let (0, held) = inst.present().map_err(|e| e.to_string())? {
                if held > 0 {
                    return Ok(Some(format!(
                        "the data ref `{ns}` has none of its {held} file(s) at {}, so `cargo mock data-refs restore {ns}` \
                         writes them again",
                        d.instance_dir(&cfg.mock_dir).display()
                    )));
                }
            }
            let tracked = d.tracked(&cfg.repo_root, &cfg.mock_dir)?;
            if tracked.is_empty() {
                return Ok(None);
            }
            let shown: Vec<&str> = tracked.iter().take(5).map(String::as_str).collect();
            let more = if tracked.len() > 5 { format!(" and {} more", tracked.len() - 5) } else { String::new() };
            Ok(Some(format!(
                "the data ref `{ns}` holds {} file(s) the branch still has tracked ({}{more}); `git rm --cached --sparse` takes \
                 them out of the tree and leaves the files",
                tracked.len(),
                shown.join(", ")
            )))
        })();
        match found {
            Ok(None) => {},
            Ok(Some(line)) => why.push(line),
            Err(e) => why.push(format!("the data ref `{ns}`: {e}")),
        }
    }
    if why.is_empty() { Ok(()) } else { Err(why) }
}
